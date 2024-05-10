use async_trait::async_trait;
use instant_acme::KeyAuthorization;
use pingora::listeners::TlsAccept;
use pingora_boringssl::{
    pkey::{PKey, Private},
    ssl::NameType,
    x509::X509,
};

use crate::StorageArc;

/// Provides the correct certificates when performing the SSL handshake
#[derive(Debug, Clone)]
pub struct CertValue {
    cert: pingora::tls::x509::X509,
    key: PKey<Private>,
}

#[derive(Debug)]
pub struct OrderPayload {
    url: String,
    key_auth: KeyAuthorization,
}

#[derive(Debug)]
pub struct CertStore {
    /// The path to the directory containing the certificates
    // certs: HashMap<String, CertValue>,
    // orders: HashMap<String, OrderPayload>,
    //
    storage: StorageArc,
}

impl CertStore {
    pub fn new(storage: StorageArc) -> Box<Self> {
        Box::new(CertStore { storage })
    }
}

#[async_trait]
impl TlsAccept for CertStore {
    /// This function is called when the SSL handshake is performed
    /// It is used to provide the correct certificate to the client
    /// based on the server name
    async fn certificate_callback(&self, ssl: &mut pingora::tls::ssl::SslRef) {
        // TODO: read the right certificate and key from the cache/filesystem
        let cert_bytes = std::fs::read("./fixtures/localhost.crt").unwrap();
        let cert = X509::from_pem(&cert_bytes).unwrap();

        let key_bytes = std::fs::read("./fixtures/localhost.key").unwrap();
        let key = PKey::private_key_from_pem(&key_bytes).unwrap();

        use pingora::tls::ext;

        // Gets the server name from the SSL connection
        // Works similarly to the HOST header in HTTP
        let host_name = ssl.servername(NameType::HOST_NAME);
        if host_name.is_none() {
            return;
        }

        let lkd = self.storage.lock().await;
        let host_cert = lkd.get_certificate("localhost");

        if host_cert.is_none() {
            println!(
                "Client sent servername: {:?}, found in cache: {:?}",
                host_name, host_cert
            )
        }

        ext::ssl_use_certificate(ssl, &cert).unwrap();
        ext::ssl_use_private_key(ssl, &key).unwrap();
    }
}
