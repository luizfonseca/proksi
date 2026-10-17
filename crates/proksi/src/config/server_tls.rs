//! `server.tls`: the TLS versions and cipher suites accepted from clients (HTTPS, and `tcp`
//! blocks terminating TLS)

use openssl::ssl::{SslContext, SslMethod, SslVersion};
use serde::{Deserialize, Serialize};

/// TLS 1.2 suites accepted by default: forward secret (ECDHE) with an AEAD, as Mozilla's
/// "intermediate" configuration. TLS 1.3 suites are all of this kind.
const DEFAULT_CIPHERS: &str = "ECDHE-ECDSA-AES128-GCM-SHA256:ECDHE-RSA-AES128-GCM-SHA256:\
ECDHE-ECDSA-AES256-GCM-SHA384:ECDHE-RSA-AES256-GCM-SHA384:\
ECDHE-ECDSA-CHACHA20-POLY1305:ECDHE-RSA-CHACHA20-POLY1305";

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ServerTls {
    /// The oldest version accepted (`"1.2"` by default, or `"1.3"`)
    pub min_version: TlsVersion,
    /// TLS 1.2 cipher suites, by their OpenSSL names, in order of preference (default: forward
    /// secret ones with an AEAD). TLS 1.3 suites are always the standard ones.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub ciphers: Vec<String>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum TlsVersion {
    #[default]
    #[serde(rename = "1.2")]
    Tls12,
    #[serde(rename = "1.3")]
    Tls13,
}

impl TlsVersion {
    pub fn ssl_version(self) -> SslVersion {
        match self {
            Self::Tls12 => SslVersion::TLS1_2,
            Self::Tls13 => SslVersion::TLS1_3,
        }
    }
}

impl ServerTls {
    /// The TLS 1.2 suites accepted, as an OpenSSL cipher list
    pub fn cipher_list(&self) -> String {
        if self.ciphers.is_empty() {
            DEFAULT_CIPHERS.to_string()
        } else {
            self.ciphers.join(":")
        }
    }

    /// Every cipher is one OpenSSL knows (it would skip the others of a list)
    pub fn validate(&self) -> Result<(), String> {
        for cipher in &self.ciphers {
            let known = SslContext::builder(SslMethod::tls())
                .and_then(|mut builder| builder.set_cipher_list(cipher))
                .is_ok();
            if !known || cipher.contains([':', ',', ' ']) {
                return Err(format!(
                    "ciphers: {cipher:?} is not a TLS 1.2 cipher suite known to OpenSSL"
                ));
            }
        }
        Ok(())
    }
}
