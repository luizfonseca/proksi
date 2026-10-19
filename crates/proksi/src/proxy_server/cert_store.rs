use async_trait::async_trait;
use openssl::ssl::{SniError, SslRef};
use pingora::listeners::TlsAccept;
use pingora::tls::ext;
use pingora::tls::ssl::NameType;

use crate::stores::{self};

/// The server name a TLS client asked for (SNI), attached to its connection once the
/// handshake completes (read by `tcp` blocks)
pub struct ServerName(pub String);

/// Provides the correct certificates when performing SSL handshakes
#[derive(Debug, Clone)]
pub struct CertStore {}

impl CertStore {
    pub fn new() -> Self {
        CertStore {}
    }

    // This function is called when the servername callback executes
    // It is used to check if the server name is in the certificate store
    // If it is, the handshake continues, otherwise it is aborted
    // and the client is disconnected
    #[allow(clippy::unnecessary_wraps)]
    pub fn sni_callback(ssl_ref: &mut SslRef) -> Result<(), SniError> {
        let servername = ssl_ref.servername(NameType::HOST_NAME).unwrap_or("");
        tracing::debug!("Received SNI: {}", servername);

        // if stores::get_certificate_by_key(servername).is_some() {
        Ok(())
        // }

        // Abort the handshake
        // Err(SniError::ALERT_FATAL)
    }
}

#[async_trait]
impl TlsAccept for CertStore {
    /// This function is called when the SSL handshake is performed
    /// It is used to provide the correct certificate to the client
    /// based on the server name
    async fn certificate_callback(&self, ssl: &mut pingora::tls::ssl::SslRef) {
        // Due to the sni_callback function, we can safely unwrap here
        let host_name = ssl
            .servername(NameType::HOST_NAME)
            .unwrap_or_default()
            .to_string();

        // A wildcard host's certificate for the names it serves (ADR 0009)
        let Some(cert) = stores::global::get_store()
            .get_certificate(&stores::lookup::certificate_name(&host_name))
            .await
        else {
            tracing::info!("No certificate found for host: {:?}", host_name);
            return;
        };

        if let Err(err) = ext::ssl_use_private_key(ssl, &cert.key) {
            tracing::error!("Failed to use private key for host {host_name:?}: {err}");
            return;
        }
        if let Err(err) = ext::ssl_use_certificate(ssl, &cert.leaf) {
            tracing::error!("Failed to use certificate for host {host_name:?}: {err}");
            return;
        }

        for chain in &cert.chain {
            if let Err(err) = ext::ssl_add_chain_cert(ssl, chain) {
                tracing::error!("Failed to add chain certificate for host {host_name:?}: {err}");
            }
        }
    }

    async fn handshake_complete_callback(
        &self,
        ssl: &pingora::tls::ssl::SslRef,
    ) -> Option<std::sync::Arc<dyn std::any::Any + Send + Sync>> {
        // Not a failed handshake (see `handshakes`)
        super::handshakes::completed(ssl);
        let name = ssl.servername(NameType::HOST_NAME)?;
        Some(std::sync::Arc::new(ServerName(name.to_lowercase())))
    }
}
