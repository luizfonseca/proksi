use async_trait::async_trait;
use pingora::listeners::TlsAccept;
use pingora_boringssl::{pkey::PKey, ssl::NameType, x509::X509};
use tracing::debug;

use crate::StorageArc;

/// Provides the correct certificates when performing the SSL handshake
#[derive(Debug)]
pub struct CertStore {
    /// The path to the directory containing the certificates
    // certs: HashMap<String, CertValue>,
    // orders: HashMap<String, OrderPayload>,
    //
    pub storage: StorageArc,
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
        use pingora::tls::ext;

        // Gets the server name from the SSL connection
        // Works similarly to the HOST header in HTTP
        let host_name = ssl.servername(NameType::HOST_NAME);
        if host_name.is_none() {
            return;
        }

        // If the server name is localhost, use the localhost certificate (disabled by default)
        if let Some(host_name) = host_name {
            if host_name == "localhost" {
                let cert_bytes = std::fs::read("./fixtures/localhost.crt").unwrap();
                let cert = X509::from_pem(&cert_bytes).unwrap();

                let key_bytes = std::fs::read("./fixtures/localhost.key").unwrap();
                let key = PKey::private_key_from_pem(&key_bytes).unwrap();

                ext::ssl_use_certificate(ssl, &cert).unwrap();
                ext::ssl_use_private_key(ssl, &key).unwrap();

                return;
            }
        }

        let cert_file = format!("./data/certificates/{}.crt", host_name.unwrap());
        let key_file = format!("./data/certificates/{}.key", host_name.unwrap());

        debug!("host {}", cert_file);

        let cert_exists = std::fs::metadata(&cert_file);
        let key_exists = std::fs::metadata(&key_file);

        if cert_exists.is_err() || key_exists.is_err() {
            debug!("nothing there");
            return;
        }

        // TODO: read the right certificate and key from the cache/filesystem
        let cert_bytes = std::fs::read(cert_file).unwrap();
        let cert = X509::from_pem(&cert_bytes).unwrap();

        let key_bytes = std::fs::read(key_file).unwrap();
        let key = PKey::private_key_from_pem(&key_bytes).unwrap();

        // let lkd = self.storage.lock().await;
        // let host_cert = lkd.get_certificate("localhost");

        // if host_cert.is_none() {
        //     println!(
        //         "Client sent servername: {:?}, found in cache: {:?}",
        //         host_name, host_cert
        //     )
        // }
        //

        ext::ssl_use_certificate(ssl, &cert).unwrap();
        ext::ssl_use_private_key(ssl, &key).unwrap();
    }
}
