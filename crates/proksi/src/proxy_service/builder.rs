// Builder pattern for creating proxy services

use std::sync::Arc;
use crate::config::Config;
use super::ProxyService;

pub struct ProxyServiceBuilder {
    server_config: Arc<Config>,
}

impl ProxyServiceBuilder {
    pub fn new(server_config: Arc<Config>) -> Self {
        Self { server_config }
    }
    
    pub fn build_services(&self) -> Vec<ProxyService> {
        let proxy_configs = if self.server_config.proxies.is_empty() {
            // Migrate from legacy config
            crate::config::migrate_server_config(&self.server_config)
        } else {
            // Use new proxy configs
            self.server_config.proxies.clone()
        };
        
        proxy_configs
            .into_iter()
            .map(|config| ProxyService::new(config, self.server_config.clone()))
            .collect()
    }
}