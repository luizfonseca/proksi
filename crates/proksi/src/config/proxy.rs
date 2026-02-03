use std::borrow::Cow;
use serde::{Deserialize, Serialize};

use super::{Route, ProtoVersion, default_proto_version, default_proto_version_min, bool_true};

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ProxyAcme {
    /// HTTP port for ACME challenges (defaults to port 80)
    pub challenge_port: Option<u16>,
    
    /// Whether to handle ACME challenges on this proxy
    #[serde(default = "bool_true")]
    pub enabled: bool,
}

impl Default for ProxyAcme {
    fn default() -> Self {
        Self {
            challenge_port: Some(80),
            enabled: true,
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ProxySsl {
    /// Whether SSL is enabled for this proxy
    #[serde(default = "bool_true")]
    pub enabled: bool,
    
    /// The minimum protocol version that the client can use.
    #[serde(
        default = "default_proto_version_min",
        deserialize_with = "super::proto_version_deser"
    )]
    pub min_proto: ProtoVersion,

    /// The maximum protocol version that the client can use.
    #[serde(
        default = "default_proto_version",
        deserialize_with = "super::proto_version_deser"
    )]
    pub max_proto: ProtoVersion,
    
    /// ACME settings (if enabled)
    pub acme: Option<ProxyAcme>,
}

impl Default for ProxySsl {
    fn default() -> Self {
        Self {
            enabled: true,
            min_proto: default_proto_version_min(),
            max_proto: default_proto_version(),
            acme: Some(ProxyAcme::default()),
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ProxyConfig {
    /// Bind address (e.g., "0.0.0.0:443", "127.0.0.1:8080")
    pub host: Cow<'static, str>,
    
    /// SSL configuration for this proxy
    pub ssl: Option<ProxySsl>,
    
    /// Routes handled by this proxy
    pub routes: Vec<Route>,
    
    /// Worker threads for this proxy (optional, defaults to 2)
    pub worker_threads: Option<usize>,
}

impl Default for ProxyConfig {
    fn default() -> Self {
        Self {
            host: Cow::Borrowed("0.0.0.0:443"),
            ssl: Some(ProxySsl::default()),
            routes: vec![],
            worker_threads: Some(2),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_proxy_config_defaults() {
        let config = ProxyConfig::default();
        assert_eq!(config.host, "0.0.0.0:443");
        assert!(config.ssl.is_some());
        assert!(config.ssl.unwrap().enabled);
        assert_eq!(config.worker_threads, Some(2));
    }
    
    #[test] 
    fn test_proxy_ssl_defaults() {
        let ssl = ProxySsl::default();
        assert!(ssl.enabled);
        assert!(ssl.acme.is_some());
        assert!(ssl.acme.unwrap().enabled);
    }
    
    #[test]
    fn test_proxy_acme_defaults() {
        let acme = ProxyAcme::default();
        assert_eq!(acme.challenge_port, Some(80));
        assert!(acme.enabled);
    }
}