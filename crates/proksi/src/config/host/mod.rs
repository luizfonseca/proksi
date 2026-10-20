//! Hosts and their locations (ADR 0003): the model every route source is converted into

use serde::{Deserialize, Serialize};

use super::RouteSslPath;

mod from_route;
mod location;
mod upstream_address;

pub use location::{
    CompiledRewrite, HealthCheck, Location, LocationTimeouts, Mirror, PathRewrite, Sticky,
    TrafficSplit, UpstreamTimeouts,
};
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
    /// Rules of every request of the host, before its location is chosen (ADR 0007)
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub rules: Vec<crate::rules::Rule>,
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
    /// How Let's Encrypt checks the host is ours, if written (see [`HostTls::challenge`])
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub challenge: Option<Challenge>,
}

impl Default for HostTls {
    fn default() -> Self {
        Self {
            enabled: true,
            certificate: None,
            self_signed_on_failure: false,
            challenge: None,
        }
    }
}

/// How Let's Encrypt checks that a host is ours (ADR 0009)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Challenge {
    /// A token served over HTTP, on port 80
    #[serde(rename = "http-01")]
    Http01,
    /// A TXT record at `_acme-challenge.<name>`, set through the DNS provider
    #[serde(rename = "dns-01")]
    Dns01,
}

impl Challenge {
    /// The challenge of the host `name`: as `written`, or DNS-01 for wildcard hosts (the only
    /// one Let's Encrypt accepts for them) and HTTP-01 for the others
    pub fn of_host(written: Option<Challenge>, name: &str) -> Challenge {
        written.unwrap_or(if name.starts_with("*.") {
            Challenge::Dns01
        } else {
            Challenge::Http01
        })
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
