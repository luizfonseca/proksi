use std::{borrow::Cow, fs::create_dir_all, path::PathBuf, sync::Arc, time::Duration};

use acme_lib::{order::NewOrder, persist::FilePersist, Account, DirectoryUrl};
use async_trait::async_trait;
use bytes::Bytes;
use dashmap::DashMap;
use pingora::{
    server::{ListenFds, ShutdownWatch},
    services::Service,
};
use tokio::time;
use tracing::{debug, info};

use crate::{config::Config, stores::certificates::Certificate, CERT_STORE};

/// A service that handles the creation of certificates using the LetsEncrypt API
pub struct LetsencryptService {
    config: Arc<Config>,
    hosts: Vec<String>,
    challenge_store: Arc<DashMap<String, (String, String)>>,
}

impl LetsencryptService {
    pub fn new(
        hosts: Vec<String>,
        config: Arc<Config>,
        store: Arc<DashMap<String, (String, String)>>,
    ) -> Self {
        Self {
            config,
            hosts,
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

    fn get_lets_encrypt_directory(&self) -> PathBuf {
        match self.config.lets_encrypt.staging {
            Some(false) => self.config.paths.lets_encrypt.join("production"),
            _ => self.config.paths.lets_encrypt.join("staging"),
        }
    }

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

            // Assuming you have a way to serve the challenge token on your web server
            // e.g., by writing it to the appropriate file path

            info!("HTTP-01 validating (retry: 5s)...");
            challenge.validate(5000)?; // Retry every 5000 ms
        }
        Ok(())
    }

    fn create_order_for_domain(
        &self,
        domain: &str,
        account: Account<FilePersist>,
    ) -> Result<(), anyhow::Error> {
        let mut order = account.new_order(domain, &[]).unwrap();

        let order_csr = loop {
            // Break if we are done confirming validations
            if let Some(csr) = order.confirm_validations() {
                break csr;
            }

            // Get the possible authorizations (for a single domain
            // this will only be one element).
            self.handle_http_01_challenge(&mut order)
                .expect("Failed to handle HTTP-01 challenge");

            order.refresh().unwrap_or_default();
        };

        // Order OK
        let pkey = acme_lib::create_p384_key();
        let order_cert = order_csr.finalize_pkey(pkey, 5000).unwrap();

        info!("Certificate created for order {:?}", order_cert.api_order());
        let cert = order_cert.download_and_save_cert().unwrap();

        let crt_bytes = Bytes::from(cert.certificate().to_string()).to_vec();
        let key_bytes = Bytes::from(cert.private_key().to_string()).to_vec();

        CERT_STORE.insert(
            Cow::Owned(domain.to_string()),
            Certificate {
                key: key_bytes,
                certificate: crt_bytes,
            },
        );
        Ok(())
    }

    async fn check_for_certificates_expiration(&self, account: &Account<FilePersist>) -> () {
        let acc = account.clone();
        let hosts = self.hosts.clone();
        let mut interval = time::interval(Duration::from_secs(84_600));
        let le_service = Self::new(
            self.hosts.clone(),
            self.config.clone(),
            self.challenge_store.clone(),
        );

        tokio::spawn(async move {
            loop {
                interval.tick().await;

                for domain in &hosts {
                    match acc.certificate(domain) {
                        Ok(Some(cert)) => {
                            let expiry = cert.valid_days_left();

                            if expiry < 5 {
                                info!(
                                    "Certificate for domain {} expires in {} days",
                                    domain, expiry
                                );
                                le_service
                                    .create_order_for_domain(domain, acc.clone())
                                    .expect("Failed to create order for domain");
                            } else {
                                debug!(
                                    "Certificate for domain {} expires in {} days",
                                    domain, expiry
                                );
                            }
                        }
                        _ => {}
                    }
                }
            }
        });
    }
}

#[async_trait]
impl Service for LetsencryptService {
    async fn start_service(&mut self, _fds: Option<ListenFds>, mut _shutdown: ShutdownWatch) {
        // Get directory based on whether we are running on staging/production
        // LetsEncrypt configurations
        let dir = self.get_lets_encrypt_directory();
        let certificates_dir = dir.as_os_str();

        info!("Creating certificates in {:?}", certificates_dir);
        // Ensure the directories exist before we start creating certificates
        create_dir_all(certificates_dir).unwrap_or_default();

        // Key-Value Store
        let persist = acme_lib::persist::FilePersist::new(certificates_dir);

        let dir = acme_lib::Directory::from_url(persist, self.get_lets_encrypt_url())
            .expect("Failed to create LetsEncrypt directory");

        let account = dir
            .account(&self.config.lets_encrypt.email)
            .expect("Failed to create or retrieve existing account");

        for domain in &self.hosts {
            match account.certificate(domain) {
                Ok(Some(cert)) => {
                    info!("Certificate already issued for domain: {}", domain);
                    let crt_bytes = Bytes::from(cert.certificate().to_string()).to_vec();
                    let key_bytes = Bytes::from(cert.private_key().to_string()).to_vec();

                    CERT_STORE.insert(
                        Cow::Owned(domain.to_string()),
                        Certificate {
                            certificate: crt_bytes,
                            key: key_bytes,
                        },
                    );
                    continue;
                }
                Ok(None) => {
                    self.create_order_for_domain(domain, account.clone())
                        .expect("Failed to create order for domain");
                }
                _ => {}
            }
        }

        // Check if any certificate needs renewal
        let every_day = self.check_for_certificates_expiration(&account);

        every_day.await;
    }

    fn name(&self) -> &'static str {
        "HttpLetsencrypt"
    }

    fn threads(&self) -> Option<usize> {
        Some(1)
    }
}
