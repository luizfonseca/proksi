use std::{fs::create_dir_all, path::PathBuf, sync::Arc, time::Duration};

use acme_lib::{order::NewOrder, persist::FilePersist, Account, DirectoryUrl};
use anyhow::anyhow;
use async_trait::async_trait;
use bytes::Bytes;
use dashmap::DashMap;
use pingora::{
    server::{ListenFds, ShutdownWatch},
    services::Service,
};
use tokio::time;
use tracing::info;

use crate::{config::Config, stores::certificates::Certificate, CERT_STORE, ROUTE_STORE};

/// A service that handles the creation of certificates using the Let's Encrypt API
pub struct LetsencryptService {
    config: Arc<Config>,
    challenge_store: Arc<DashMap<String, (String, String)>>,
}

impl LetsencryptService {
    pub fn new(config: Arc<Config>, store: Arc<DashMap<String, (String, String)>>) -> Self {
        Self {
            config,
            challenge_store: store,
        }
    }

    // Based on the letsencrypt configuration, return the appropriate URL
    fn get_lets_encrypt_url(&self) -> DirectoryUrl {
        match self.config.lets_encrypt.staging {
            Some(false) => DirectoryUrl::LetsEncrypt,
            _ => DirectoryUrl::LetsEncryptStaging,
        }
    }

    /// Return the appropriate Let's Encrypt directories for certificates based on the environment
    fn get_lets_encrypt_directory(&self) -> PathBuf {
        match self.config.lets_encrypt.staging {
            Some(false) => self.config.paths.lets_encrypt.join("production"),
            _ => self.config.paths.lets_encrypt.join("staging"),
        }
    }

    /// Start an HTTP-01 challenge for a given order
    fn handle_http_01_challenge(
        &self,
        order: &mut NewOrder<FilePersist>,
    ) -> Result<(), anyhow::Error> {
        for auth in order.authorizations()? {
            let challenge = auth.http_challenge();

            info!("HTTP-01 challenge for domain: {}", auth.domain_name());
            self.challenge_store.insert(
                auth.domain_name().to_string(),
                (
                    challenge.http_token().to_string(),
                    challenge.http_proof().to_string(),
                ),
            );

            // Let's Encrypt will check the domain's URL to validate the challenge
            info!("HTTP-01 validating (retry: 5s)...");
            challenge.validate(5000)?; // Retry every 5000 ms
        }
        Ok(())
    }

    /// Create a new order for a domain (HTTP-01 challenge)
    fn create_order_for_domain(
        &self,
        domain: &str,
        account: &Account<FilePersist>,
    ) -> Result<bool, anyhow::Error> {
        let mut order = account.new_order(domain, &[])?;

        let order_csr = loop {
            // Break if we are done confirming validations
            if let Some(csr) = order.confirm_validations() {
                break csr;
            }

            // Get the possible authorizations (for a single domain
            // this will only be one element).
            self.handle_http_01_challenge(&mut order)
                .map_err(|err| anyhow!("Failed to handle HTTP-01 challenge: {err}"))?;

            order.refresh().unwrap_or_default();
        };

        // Order OK
        let pkey = acme_lib::create_p384_key();
        let order_cert = order_csr.finalize_pkey(pkey, 5000)?;

        info!("Certificate created for order {:?}", order_cert.api_order());
        let cert = order_cert.download_and_save_cert()?;

        let crt_bytes = Bytes::from(cert.certificate().to_string()).to_vec();
        let key_bytes = Bytes::from(cert.private_key().to_string()).to_vec();

        CERT_STORE.insert(
            domain.to_string(),
            Certificate {
                key: key_bytes,
                certificate: crt_bytes,
            },
        );

        Ok(true)
    }

    /// Watch for route changes and create or update certificates for new routes
    fn watch_for_route_changes(
        &self,
        account: &Account<FilePersist>,
    ) -> tokio::task::JoinHandle<()> {
        let acc = account.clone();
        let mut interval = time::interval(Duration::from_secs(30));
        let service = Self::new(self.config.clone(), self.challenge_store.clone());

        tokio::spawn(async move {
            loop {
                interval.tick().await;
                for route in ROUTE_STORE.iter() {
                    if CERT_STORE.contains_key(route.key()) {
                        continue;
                    }

                    service.handle_certificate_for_domain(route.key(), &acc);
                }
            }
        })
    }

    /// Check for certificates expiration and renew them if needed
    fn check_for_certificates_expiration(
        &self,
        account: &Account<FilePersist>,
    ) -> tokio::task::JoinHandle<()> {
        let acc = account.clone();
        let mut interval = time::interval(Duration::from_secs(84_600));
        let service = Self::new(self.config.clone(), self.challenge_store.clone());

        tokio::spawn(async move {
            loop {
                interval.tick().await;
                for value in ROUTE_STORE.iter() {
                    let domain = value.key();
                    let cert = acc.certificate(domain).unwrap();

                    if cert.is_none() {
                        continue;
                    }

                    let valid_days_left = cert.unwrap().valid_days_left();
                    info!("Certificate for domain {domain} expires in {valid_days_left} days",);

                    // Nothing to do
                    if valid_days_left > 5 {
                        continue;
                    }

                    service
                        .create_order_for_domain(domain, &acc)
                        .map_err(|e| anyhow!("Failed to create order for {domain}: {e}"))
                        .unwrap();
                }
            }
        })
    }

    fn handle_certificate_for_domain(&self, domain: &str, account: &Account<FilePersist>) {
        match account.certificate(domain) {
            Ok(Some(cert)) => {
                // Certificate already exists
                if CERT_STORE.contains_key(domain) {
                    return;
                }

                let crt_bytes = Bytes::from(cert.certificate().to_string()).to_vec();
                let key_bytes = Bytes::from(cert.private_key().to_string()).to_vec();

                CERT_STORE.insert(
                    domain.to_string(),
                    Certificate {
                        certificate: crt_bytes,
                        key: key_bytes,
                    },
                );
            }
            Ok(None) => {
                // TODO create self signed certificate if no certificate is found
                self.create_order_for_domain(domain, account)
                    .expect("Failed to create order for domain");
            }
            _ => {}
        }
    }
}

#[async_trait]
impl Service for LetsencryptService {
    async fn start_service(&mut self, _fds: Option<ListenFds>, mut _shutdown: ShutdownWatch) {
        info!(service = "letsencrypt", "Started LetsEncrypt service");

        // Get directory based on whether we are running on staging/production
        // LetsEncrypt configurations
        let dir = self.get_lets_encrypt_directory();
        let certificates_dir = dir.as_os_str();

        info!("Creating certificates in {certificates_dir:?}");
        // Ensure the directories exist before we start creating certificates
        create_dir_all(certificates_dir).unwrap_or_default();

        // Key-Value Store
        let persist = acme_lib::persist::FilePersist::new(certificates_dir);

        let dir = acme_lib::Directory::from_url(persist, self.get_lets_encrypt_url())
            .expect("Failed to create LetsEncrypt directory");

        let account = dir
            .account(&self.config.lets_encrypt.email)
            .expect("Failed to create or retrieve existing account");

        tokio::select! {
            _ = self.watch_for_route_changes(&account) => {}
            _ = self.check_for_certificates_expiration(&account) => {}
        }
    }

    fn name(&self) -> &'static str {
        "lets_encrypt_service"
    }

    fn threads(&self) -> Option<usize> {
        Some(1)
    }
}
