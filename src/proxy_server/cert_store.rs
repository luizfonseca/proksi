use async_trait::async_trait;
use openssl::ssl::{SniError, SslRef};
use pingora::listeners::TlsAccept;
use pingora_openssl::{ext, pkey::PKey, ssl::NameType, x509::X509};

use crate::{stores::certificates::CertificateStore, CERT_STORE};

/// Provides the correct certificates when performing SSL handshakes
#[derive(Debug)]
pub struct CertStore {}

impl CertStore {
    pub fn new() -> Self {
        CertStore {}
    }

    // This function is called when the servername callback executes
    // It is used to check if the server name is in the certificate store
    // If it is, the handshake continues, otherwise it is aborted
    // and the client is disconnected
    pub fn sni_callback(ssl_ref: &mut SslRef, store: &CertificateStore) -> Result<(), SniError> {
        let servername = ssl_ref.servername(NameType::HOST_NAME).unwrap_or("");
        tracing::debug!("Received SNI: {}", servername);

        if store.get(servername).is_some() {
            return Ok(());
        }

        // Abort the handshake
        Err(SniError::ALERT_FATAL)
    }
}

#[async_trait]
impl TlsAccept for CertStore {
    /// This function is called when the SSL handshake is performed
    /// It is used to provide the correct certificate to the client
    /// based on the server name
    async fn certificate_callback(&self, ssl: &mut pingora::tls::ssl::SslRef) {
        // Due to the sni_callback function, we can safely unwrap here
        let host_name = ssl.servername(NameType::HOST_NAME);
        let certificate = CERT_STORE.get(host_name.unwrap_or(""));
        if certificate.is_none() {
            tracing::debug!("No certificate found for host: {:?}", host_name);
            return;
        }

        // Data from DashMap
        let result = certificate.unwrap();
        let cert = &result.value();

        let crt_value = X509::from_pem(&cert.certificate).unwrap();
        let key_value = PKey::private_key_from_pem(&cert.key).unwrap();

        ext::ssl_use_certificate(ssl, &crt_value).unwrap();
        ext::ssl_use_private_key(ssl, &key_value).unwrap();
    }
}
