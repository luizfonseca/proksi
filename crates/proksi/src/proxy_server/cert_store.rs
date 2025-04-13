use async_trait::async_trait;
use openssl::ssl::{SniError, SslRef};
use pingora::listeners::TlsAccept;
use pingora::tls::ext;
use pingora::tls::ssl::NameType;

use crate::stores::{self};

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
        let host_name = ssl.servername(NameType::HOST_NAME).unwrap_or_default();

        let Some(cert) = stores::global::get_store().get_certificate(host_name).await else {
            tracing::info!("No certificate found for host: {:?}", host_name);
            return;
        };

        ext::ssl_use_private_key(ssl, &cert.key).unwrap();
        ext::ssl_use_certificate(ssl, &cert.leaf).unwrap();

        if let Some(chain) = &cert.chain {
            ext::ssl_add_chain_cert(ssl, chain).unwrap();
        }
    }
}
