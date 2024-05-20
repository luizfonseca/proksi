use async_trait::async_trait;
use pingora::listeners::TlsAccept;
use pingora_openssl::{pkey::PKey, ssl::NameType, x509::X509};
use tracing::debug;

use crate::CERT_STORE;

/// Provides the correct certificates when performing the SSL handshake
#[derive(Debug)]
pub struct CertStore {}

impl CertStore {
    pub fn new() -> Self {
        CertStore {}
    }
}

#[async_trait]
impl TlsAccept for CertStore {
    /// This function is called when the SSL handshake is performed
    /// It is used to provide the correct certificate to the client
    /// based on the server name
    async fn certificate_callback(&self, ssl: &mut pingora::tls::ssl::SslRef) {
        use pingora::tls::ext;

        // Gets the server name from the SSL connection
        // Works similarly to the HOST header in HTTP
        let host_name = ssl.servername(NameType::HOST_NAME);
        if host_name.is_none() {
            debug!("No servername for HTTPS request, aborting...");
            return;
        }

        let certificate = CERT_STORE.get(host_name.unwrap());
        if certificate.is_none() {
            debug!("No certificate found for host: {:?}", host_name);
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
