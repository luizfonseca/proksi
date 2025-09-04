use std::borrow::Cow;
use crate::config::{Config, ProtoVersion};
use super::{ProxyConfig, ProxySsl, ProxyAcme};

/// Migrates old ServerCfg format to new ProxyConfig format
pub fn migrate_server_config(config: &Config) -> Vec<ProxyConfig> {
    // If proxies are already configured, use them
    if !config.proxies.is_empty() {
        return config.proxies.clone();
    }
    
    // Otherwise, migrate from old ServerCfg format
    let server = &config.server;
    
    let https_address = server.https_address.clone()
        .unwrap_or_else(|| Cow::Borrowed("0.0.0.0:443"));
    let http_address = server.http_address.clone()
        .unwrap_or_else(|| Cow::Borrowed("0.0.0.0:80"));
    
    let mut proxies = Vec::new();
    
    if server.ssl_enabled {
        // Create HTTPS proxy with SSL enabled
        let https_proxy = ProxyConfig {
            host: https_address,
            ssl: Some(ProxySsl {
                enabled: true,
                min_proto: ProtoVersion::V1_2,
                max_proto: ProtoVersion::V1_3,
                acme: Some(ProxyAcme {
                    challenge_port: extract_port(&http_address),
                    enabled: true,
                }),
            }),
            routes: config.routes.clone(),
            worker_threads: config.worker_threads,
        };
        
        proxies.push(https_proxy);
        
        // Create HTTP proxy for ACME challenges only
        let http_proxy = ProxyConfig {
            host: http_address,
            ssl: None, // No SSL for ACME challenges
            routes: vec![], // No routes, only ACME challenges
            worker_threads: Some(1), // Minimal threads for ACME
        };
        
        proxies.push(http_proxy);
    } else {
        // Create HTTP-only proxy (SSL disabled mode)
        let http_proxy = ProxyConfig {
            host: https_address.clone(), // Clone to avoid move
            ssl: None,
            routes: config.routes.clone(),
            worker_threads: config.worker_threads,
        };
        
        proxies.push(http_proxy);
        
        // Also create service on http_address if different
        if https_address.as_ref() != http_address.as_ref() {
            let secondary_proxy = ProxyConfig {
                host: http_address,
                ssl: None,
                routes: config.routes.clone(),
                worker_threads: config.worker_threads,
            };
            
            proxies.push(secondary_proxy);
        }
    }
    
    proxies
}

/// Extracts port number from address string
fn extract_port(address: &str) -> Option<u16> {
    address.split(':')
        .last()
        .and_then(|port_str| port_str.parse().ok())
}

/// Checks if configuration uses old format
pub fn is_legacy_config(config: &Config) -> bool {
    config.proxies.is_empty()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use std::borrow::Cow;
    
    #[test]
    fn test_migrate_ssl_enabled() {
        let config = Config {
            server: crate::config::ServerCfg {
                https_address: Some(Cow::Borrowed("0.0.0.0:443")),
                http_address: Some(Cow::Borrowed("0.0.0.0:80")),
                ssl_enabled: true,
                ssl_disabled_flag: false,
                ssl_enabled_flag: false,
            },
            proxies: vec![],
            routes: vec![],
            worker_threads: Some(4),
            ..Default::default()
        };
        
        let proxies = migrate_server_config(&config);
        
        assert_eq!(proxies.len(), 2);
        
        // HTTPS proxy
        assert_eq!(proxies[0].host, "0.0.0.0:443");
        assert!(proxies[0].ssl.is_some());
        assert!(proxies[0].ssl.as_ref().unwrap().enabled);
        assert_eq!(proxies[0].worker_threads, Some(4));
        
        // HTTP proxy for ACME
        assert_eq!(proxies[1].host, "0.0.0.0:80");
        assert!(proxies[1].ssl.is_none());
        assert_eq!(proxies[1].worker_threads, Some(1));
    }
    
    #[test]
    fn test_migrate_ssl_disabled() {
        let config = Config {
            server: crate::config::ServerCfg {
                https_address: Some(Cow::Borrowed("0.0.0.0:8080")),
                http_address: Some(Cow::Borrowed("0.0.0.0:8081")),
                ssl_enabled: false,
                ssl_disabled_flag: false,
                ssl_enabled_flag: false,
            },
            proxies: vec![],
            routes: vec![],
            worker_threads: Some(2),
            ..Default::default()
        };
        
        let proxies = migrate_server_config(&config);
        
        assert_eq!(proxies.len(), 2);
        
        // Primary HTTP proxy
        assert_eq!(proxies[0].host, "0.0.0.0:8080");
        assert!(proxies[0].ssl.is_none());
        assert_eq!(proxies[0].worker_threads, Some(2));
        
        // Secondary HTTP proxy
        assert_eq!(proxies[1].host, "0.0.0.0:8081");
        assert!(proxies[1].ssl.is_none());
        assert_eq!(proxies[1].worker_threads, Some(2));
    }
    
    #[test]
    fn test_is_legacy_config() {
        let legacy_config = Config {
            proxies: vec![],
            ..Default::default()
        };
        
        assert!(is_legacy_config(&legacy_config));
        
        let new_config = Config {
            proxies: vec![ProxyConfig::default()],
            ..Default::default()
        };
        
        assert!(!is_legacy_config(&new_config));
    }
    
    #[test]
    fn test_extract_port() {
        assert_eq!(extract_port("0.0.0.0:443"), Some(443));
        assert_eq!(extract_port("127.0.0.1:8080"), Some(8080));
        assert_eq!(extract_port("localhost:3000"), Some(3000));
        assert_eq!(extract_port("invalid"), None);
    }
}