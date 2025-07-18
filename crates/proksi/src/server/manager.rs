use crate::config::{Config, ProxyConfig};

pub fn log_proxy_configuration(config: &Config) {
    let proxies = if config.proxies.is_empty() {
        crate::config::migrate_server_config(config)
    } else {
        config.proxies.clone()
    };
    
    tracing::info!("Proxy configuration loaded:");
    for (i, proxy) in proxies.iter().enumerate() {
        let ssl_status = if let Some(ssl) = &proxy.ssl {
            if ssl.enabled {
                "SSL enabled"
            } else {
                "SSL disabled"
            }
        } else {
            "HTTP only"
        };
        
        tracing::info!(
            "  Proxy {}: {} ({}, {} worker threads)",
            i + 1,
            proxy.host,
            ssl_status,
            proxy.worker_threads.unwrap_or(2)
        );
    }
}

pub fn validate_proxy_configs(proxies: &[ProxyConfig]) -> Result<(), String> {
    if proxies.is_empty() {
        return Err("At least one proxy configuration is required".to_string());
    }
    
    // Check for duplicate host addresses
    let mut hosts = std::collections::HashSet::new();
    for proxy in proxies {
        if !hosts.insert(proxy.host.as_ref()) {
            return Err(format!("Duplicate proxy host address: {}", proxy.host));
        }
    }
    
    Ok(())
}