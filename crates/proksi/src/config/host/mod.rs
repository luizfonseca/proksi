//! Hosts and their locations (ADR 0003): the model every route source is converted into

use serde::{Deserialize, Serialize};

use super::RouteSslPath;

mod from_route;
mod location;
mod upstream_address;

pub use location::{Location, LocationTimeouts};
pub use upstream_address::parse_upstream_address;

/// A host: its name, TLS settings and the locations serving its requests
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Host {
    pub name: String,
    /// Addresses the host is served on (`ip:port`). Empty: the default HTTPS address
    /// (`server.https_address`), or the default HTTP address with TLS disabled.
    #[serde(default)]
    pub listen: Vec<String>,
    #[serde(default)]
    pub tls: HostTls,
    #[serde(default)]
    pub locations: Vec<Location>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HostTls {
    /// Serve the host over HTTPS (`false`: plain HTTP on the HTTP port, without certificates)
    #[serde(default = "enabled_by_default")]
    pub enabled: bool,
    /// Certificate files, used instead of Let's Encrypt
    pub certificate: Option<RouteSslPath>,
    /// Use a self-signed certificate if no other certificate can be obtained
    #[serde(default)]
    pub self_signed_on_failure: bool,
}

impl Default for HostTls {
    fn default() -> Self {
        Self {
            enabled: true,
            certificate: None,
            self_signed_on_failure: false,
        }
    }
}

fn enabled_by_default() -> bool {
    true
}

#[cfg(test)]
mod tests {
    use super::Host;

    #[test]
    fn test_tls_is_enabled_by_default() {
        let host: Host = serde_json::from_value(serde_json::json!({
            "name": "a.test",
            "tls": { "self_signed_on_failure": true },
        }))
        .unwrap();
        assert!(host.tls.enabled);

        let host: Host = serde_json::from_value(serde_json::json!({ "name": "a.test" })).unwrap();
        assert!(host.tls.enabled);
        // Without addresses, the host is served on the default ones
        assert!(host.listen.is_empty());
    }
}
