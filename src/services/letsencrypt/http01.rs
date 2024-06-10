use std::{fs::create_dir_all, path::PathBuf, sync::Arc, time::Duration};

use acme_lib::{order::NewOrder, persist::FilePersist, Account, DirectoryUrl};
use anyhow::anyhow;
use async_trait::async_trait;
use dashmap::DashMap;
use openssl::{pkey::PKey, x509::X509};
use pingora::{
    server::{ListenFds, ShutdownWatch},
    services::Service,
};
use tokio::time;
use tracing::info;

use crate::{
    config::Config,
    stores::{
        certificates::{Certificate, CertificateStore},
        routes::RouteStore,
    },
};

/// A service that handles the creation of certificates using the Let's Encrypt API
#[derive(Clone)]
pub struct LetsencryptService {
    pub(crate) config: Arc<Config>,
    pub(crate) challenge_store: Arc<DashMap<String, (String, String)>>,
    pub(crate) route_store: RouteStore,
    pub(crate) cert_store: CertificateStore,
}

impl LetsencryptService {
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
    ) -> Result<(), anyhow::Error> {
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

        info!("certificate created for order {:?}", order_cert.api_order());
        let cert = order_cert.download_and_save_cert()?;

        let crt_bytes = X509::from_pem(cert.certificate().as_bytes())?;
        let key_bytes = PKey::private_key_from_pem(cert.private_key().as_bytes())?;

        self.cert_store.insert(
            domain.to_string(),
            Certificate {
                key: key_bytes,
                certificate: crt_bytes,
            },
        );

        Ok(())
    }

    /// Watch for route changes and create or update certificates for new routes
    fn watch_for_route_changes(
        &self,
        account: &Account<FilePersist>,
    ) -> tokio::task::JoinHandle<()> {
        let acc = account.clone();
        let mut interval = time::interval(Duration::from_secs(30));
        let service = self.clone();

        tokio::spawn(async move {
            loop {
                interval.tick().await;
                for route in service.route_store.iter() {
                    if service.cert_store.contains_key(route.key()) {
                        continue;
                    }

                    service.handle_certificate_for_domain(
                        route.key(),
                        &acc,
                        route.self_signed_certificate,
                    );
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
        let service = self.clone();

        tokio::spawn(async move {
            loop {
                interval.tick().await;
                for value in service.route_store.iter() {
                    let domain = value.key();
                    let cert = acc.certificate(domain).unwrap();

                    if cert.is_none() {
                        continue;
                    }

                    let valid_days_left = cert.unwrap().valid_days_left();
                    info!("certificate for domain {domain} expires in {valid_days_left} days",);

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

    fn handle_certificate_for_domain(
        &self,
        domain: &str,
        account: &Account<FilePersist>,
        self_signed_on_failure: bool,
    ) {
        match account.certificate(domain) {
            Ok(Some(cert)) => {
                // Certificate already exists
                if self.cert_store.contains_key(domain) {
                    return;
                }

                let crt = X509::from_pem(cert.certificate().as_bytes()).unwrap();
                let key = PKey::private_key_from_pem(cert.private_key().as_bytes()).unwrap();

                self.cert_store.insert(
                    domain.to_string(),
                    Certificate {
                        certificate: crt,
                        key,
                    },
                );
            }
            Ok(None) => {
                // TODO create self signed certificate if no certificate is found
                let order = self.create_order_for_domain(domain, account);
                if order.is_err() {
                    // Create self signed certificate
                    self.create_self_signed_certificate(domain, self_signed_on_failure)
                        .unwrap();
                }
            }
            _ => {}
        }
    }

    /// Creates an in-memory self-signed certificate for a domain if let's encrypt
    /// cannot be used.
    /// Note this is only useful for local development or testing purposes
    /// and should be used sparingly
    fn create_self_signed_certificate(
        &self,
        domain: &str,
        enabled: bool,
    ) -> Result<(), anyhow::Error> {
        // Generate self-signed certificate only if self_signed_on_failure is set to true
        // If not provided, default to true
        if !enabled {
            // Nothing to do
            return Ok(());
        }

        tracing::info!("creating an in-memory self-signed certificate for {domain}");

        let rsa = openssl::rsa::Rsa::generate(2048)?;
        let mut openssl_cert = openssl::x509::X509Builder::new()?;
        let mut x509_name = openssl::x509::X509NameBuilder::new()?;

        x509_name.append_entry_by_text("CN", domain)?;
        x509_name.append_entry_by_text("ST", "TX")?;
        x509_name.append_entry_by_text("O", "Proksi")?;
        x509_name.append_entry_by_text("CN", "Test")?;
        let x509_name = x509_name.build();

        let hash = pingora_openssl::hash::MessageDigest::sha256();
        let key = pingora_openssl::pkey::PKey::from_rsa(rsa)?;
        let one_year = openssl::asn1::Asn1Time::days_from_now(365)?;
        let today = openssl::asn1::Asn1Time::days_from_now(0)?;
        openssl_cert.set_version(2)?;
        openssl_cert.set_subject_name(&x509_name)?;
        openssl_cert.set_issuer_name(&x509_name)?;
        openssl_cert.set_pubkey(&key)?;
        openssl_cert.set_not_before(&today)?;
        openssl_cert.set_not_after(&one_year)?;
        openssl_cert.sign(&key, hash)?;

        let openssl_cert = openssl_cert.build();

        self.cert_store.insert(
            domain.to_string(),
            Certificate {
                key,
                certificate: openssl_cert,
            },
        );

        Ok(())
    }
}

#[async_trait]
impl Service for LetsencryptService {
    async fn start_service(&mut self, _fds: Option<ListenFds>, mut _shutdown: ShutdownWatch) {
        info!("started LetsEncrypt service");

        // Get directory based on whether we are running on staging/production
        // LetsEncrypt configurations
        let dir = self.get_lets_encrypt_directory();
        let certificates_dir = dir.as_os_str();

        info!("creating certificates in {certificates_dir:?}");
        // Ensure the directories exist before we start creating certificates
        create_dir_all(certificates_dir).unwrap_or_default();

        // Key-Value Store
        let persist = acme_lib::persist::FilePersist::new(certificates_dir);

        let dir = acme_lib::Directory::from_url(persist, self.get_lets_encrypt_url())
            .expect("failed to create LetsEncrypt directory");

        let account = dir
            .account(&self.config.lets_encrypt.email)
            .expect("failed to create or retrieve existing account");

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
