use std::{sync::Arc, time::Duration};

use acme_v2::{order::NewOrder, Account, DirectoryUrl};
use anyhow::anyhow;
use async_trait::async_trait;

use openssl::{pkey::PKey, x509::X509};
use pingora::{
    server::{ListenFds, ShutdownWatch},
    services::Service,
};
use tokio::time;
use tracing::info;

use crate::{
    config::Config,
    stores::{self, certificates::Certificate},
};

use super::storage::PersistType;

/// Default interval in days to attempt renewal of certificates
const DEFAULT_RENEW_INTERVAL_DAYS: i64 = 30;

/// A service that handles the creation of certificates using the Let's Encrypt API
pub struct LetsencryptService {
    pub(crate) config: Arc<Config>,
    // pub(crate) route_store: RouteStore,
    // pub(crate) cert_store: CertificateStore,
}

impl LetsencryptService {
    pub fn new(config: Arc<Config>) -> Self {
        Self { config }
    }

    /// Parse a PEM-encoded X509 certificate from a string slice
    fn parse_x509_cert(cert_pem: &str) -> Result<X509, anyhow::Error> {
        Ok(X509::from_pem(cert_pem.as_bytes())?)
    }

    /// Parse a PEM-encoded private key from a string slice
    fn parse_private_key(key_pem: &str) -> Result<PKey<openssl::pkey::Private>, anyhow::Error> {
        Ok(PKey::private_key_from_pem(key_pem.as_bytes())?)
    }

    /// Update global certificate store with new `X509` and `PKey` for the
    /// given domain also considering that the certificate could be a bundle file.
    async fn insert_certificate(
        domain: &str,
        bundle: &str,
        key_pem: &str,
    ) -> Result<(), anyhow::Error> {
        let end = "-----END CERTIFICATE-----";
        // Split certificates (leaf and chain) from the bundle
        let split = bundle
            .split_inclusive(end)
            .map(str::trim)
            .collect::<Vec<&str>>();

        let leaf_pem = split.first();
        let chain_pem = split.get(1);

        let Some(leaf) = leaf_pem else {
            return Err(anyhow::anyhow!("Certificate is empty"));
        };
        let leaf = Self::parse_x509_cert(leaf)?;
        let mut chain: Option<X509> = None;

        if let Some(chain_pem) = chain_pem {
            tracing::trace!("chain PEM: {:?}", chain_pem);
            chain = Some(Self::parse_x509_cert(chain_pem)?);
        }

        let key = Self::parse_private_key(key_pem)?;

        stores::global::get_store()
            .set_certificate(domain, Certificate { key, leaf, chain })
            .await
            .map_err(|o_err| anyhow!("failed to save certificate in the store: {}", o_err))?;

        Ok(())
    }

    /// Start an HTTP-01 challenge for a given order
    async fn handle_http_01_challenge(
        order: &mut NewOrder<PersistType>,
    ) -> Result<(), anyhow::Error> {
        for auth in order.authorizations()? {
            let challenge = auth.http_challenge();

            info!("HTTP-01 challenge for domain: {}", auth.domain_name());

            // Use the global store to set the challenge
            // This allows multiple replicas of Proksi to handle challenges
            if let Err(err) = stores::global::get_store()
                .set_challenge(
                    auth.domain_name(),
                    challenge.http_token().to_string(),
                    challenge.http_proof().to_string(),
                )
                .await
            {
                tracing::error!("Failed to set challenge in store: {}", err);
                return Err(anyhow!("Failed to set challenge in store: {}", err));
            }

            // Let's Encrypt will check the domain's URL to validate the challenge
            tracing::info!("HTTP-01 validating (retry: 5s)...");
            challenge.validate(5000)?; // Retry every 5000 ms
        }
        Ok(())
    }

    /// Creates an in-memory self-signed certificate for a domain if let's encrypt
    /// cannot be used.
    /// Note this is only useful for local development or testing purposes
    /// and should be used sparingly
    async fn create_self_signed_certificate(
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
        // x509_name.append_entry_by_text("CN", "Test")?;
        let x509_name = x509_name.build();

        let hash = pingora::tls::hash::MessageDigest::sha256();
        let key = pingora::tls::pkey::PKey::from_rsa(rsa)?;
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

        stores::global::get_store()
            .set_certificate(
                domain,
                Certificate {
                    key,
                    leaf: openssl_cert,
                    chain: None,
                },
            )
            .await
            .map_err(|o_err| anyhow!("failed to save self-signed certificate {}", o_err))?;

        Ok(())
    }

    // Based on the letsencrypt configuration, return the appropriate URL
    fn get_lets_encrypt_url(&self) -> DirectoryUrl {
        match self.config.lets_encrypt.staging {
            Some(false) => DirectoryUrl::LetsEncrypt,
            _ => DirectoryUrl::LetsEncryptStaging,
        }
    }

    /// Create a new order for a domain (HTTP-01 challenge)
    async fn create_order_for_domain(
        domain: &str,
        account: &Account<PersistType>,
    ) -> Result<(), anyhow::Error> {
        let mut order = account.new_order(domain, &[])?;

        let order_csr = loop {
            // Break if we are done confirming validations
            if let Some(csr) = order.confirm_validations() {
                break csr;
            }

            // Get the possible authorizations (for a single domain
            // this will only be one element).
            Self::handle_http_01_challenge(&mut order)
                .await
                .map_err(|err| anyhow!("Failed to handle HTTP-01 challenge: {err}"))?;

            order.refresh().unwrap_or_default();
        };

        // Order OK
        let pkey = acme_v2::create_p384_key();
        let order_cert = order_csr.finalize_pkey(pkey, 5000)?;

        info!("certificate created for order {:?}", order_cert.api_order());

        let cert = order_cert.download_and_save_cert()?;

        Self::insert_certificate(domain, cert.certificate(), cert.private_key()).await?;

        Ok(())
    }

    /// Watch for route changes and create or update certificates for new routes
    async fn watch_for_route_changes(&self, account: &Account<PersistType>) {
        let mut interval = time::interval(Duration::from_secs(20));

        loop {
            interval.tick().await;
            tracing::debug!("checking for new routes to create certificates for");
            for (key, value) in &stores::get_routes() {
                if stores::global::get_store()
                    .get_certificates()
                    .await
                    .contains_key(key)
                {
                    continue;
                }

                Self::handle_certificate_for_domain(key, account, value.self_signed_certificate)
                    .await;
            }
        }
    }

    /// Check for certificates expiration and renew them if needed
    async fn check_for_certificates_expiration(&self, account: &Account<PersistType>) {
        let mut interval = time::interval(Duration::from_secs(
            self.config
                .lets_encrypt
                .renew_interval_secs
                .unwrap_or(84_600),
        ));

        loop {
            tracing::debug!("checking for certificates to renew");
            for (domain, _) in &stores::get_routes() {
                let Ok(Some(cert)) = account.certificate(domain) else {
                    continue;
                };

                let valid_days_left = cert.valid_days_left();
                tracing::info!("certificate for domain {domain} expires in {valid_days_left} days",);

                // Nothing to do before the renewal interval
                if valid_days_left > DEFAULT_RENEW_INTERVAL_DAYS {
                    continue;
                }

                tracing::info!("trying to renew certificate for domain: {domain}");
                if let Err(error) = Self::create_order_for_domain(domain, account)
                    .await
                    .map_err(|e| anyhow!("Failed to create order for {domain}: {e}"))
                {
                    tracing::error!("failed to renew certificate for domain {domain}: {error}");
                }
            }

            interval.tick().await;
        }
    }

    async fn handle_certificate_for_domain(
        domain: &str,
        account: &Account<PersistType>,
        self_signed_on_failure: bool,
    ) {
        match account.certificate(domain) {
            Ok(Some(cert)) => {
                // Certificate already exists
                if stores::global::get_store()
                    .get_certificates()
                    .await
                    .contains_key(domain)
                {
                    return;
                }

                if let Err(err) =
                    Self::insert_certificate(domain, cert.certificate(), cert.private_key()).await
                {
                    tracing::error!("failed to insert certificate for domain {domain}: {err}");
                };
            }
            Ok(None) => {
                if Self::create_order_for_domain(domain, account)
                    .await
                    .is_err()
                {
                    Self::create_self_signed_certificate(domain, self_signed_on_failure)
                        .await
                        .ok();
                }
            }
            _ => {}
        }
    }
}

#[async_trait]
impl Service for LetsencryptService {
    async fn start_service(
        &mut self,
        _fds: Option<ListenFds>,
        mut _shutdown: ShutdownWatch,
        _listeners_per_fd: usize,
    ) {
        if self.config.lets_encrypt.enabled.is_some_and(|v| !v) {
            // Nothing to do, lets encrypt is disabled
            return;
        }
        info!("started LetsEncrypt service");

        let persist =
            crate::services::letsencrypt::storage::CertificatePersist::new(self.config.clone());

        let dir = acme_v2::Directory::from_url(persist.get_persist(), self.get_lets_encrypt_url())
            .expect("failed to create LetsEncrypt directory");

        let account = dir
            .account(&self.config.lets_encrypt.email)
            .expect("failed to create or retrieve existing account");

        let _ = tokio::join!(
            self.watch_for_route_changes(&account),
            self.check_for_certificates_expiration(&account)
        );
    }

    fn name(&self) -> &'static str {
        "lets_encrypt_service"
    }

    fn threads(&self) -> Option<usize> {
        Some(1)
    }
}
