use async_trait::async_trait;
use openssl::ssl::{SniError, SslAlert, SslRef};
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

    /// The name a client asks for (SNI): handshakes for names no host with TLS serves (or none)
    /// end here, with `unrecognized_name`, before a certificate is looked for. Scanners and
    /// names pointed at the server by mistake cost nothing, and don't reach the store.
    pub fn sni_callback(ssl_ref: &mut SslRef, alert: &mut SslAlert) -> Result<(), SniError> {
        let name = ssl_ref.servername(NameType::HOST_NAME).unwrap_or_default();
        let served = !name.is_empty()
            && stores::lookup::named_host(name).is_some_and(|host| host.tls_enabled);
        if served {
            return Ok(());
        }
        tracing::debug!("TLS handshake for {name:?}, which no host with TLS serves: refused");
        *alert = SslAlert::UNRECOGNIZED_NAME;
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
        let host_name = ssl
            .servername(NameType::HOST_NAME)
            .unwrap_or_default()
            .to_string();

        // A wildcard host's certificate for the names it serves (ADR 0009)
        let Some(cert) = stores::global::get_store()
            .get_certificate(&stores::lookup::certificate_name(&host_name))
            .await
        else {
            // A host with TLS, whose certificate isn't there yet (or failed to be issued)
            tracing::warn!("no certificate for host {host_name:?}");
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
