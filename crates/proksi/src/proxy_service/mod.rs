use std::sync::Arc;
use pingora::proxy::http_proxy_service;
use pingora::server::Server;
use pingora::listeners::tls::TlsSettings;
use crate::config::{Config, ProxyConfig};
use crate::proxy_server::{https_proxy::Router, http_proxy::HttpLB, cert_store::CertStore};
use anyhow::Result;

pub mod ssl;
pub mod acme;
pub mod builder;

pub struct ProxyService {
    config: ProxyConfig,
    server_config: Arc<Config>,
}

impl ProxyService {
    pub fn new(config: ProxyConfig, server_config: Arc<Config>) -> Self {
        Self {
            config,
            server_config,
        }
    }
    
    pub fn add_to_server(&self, server: &mut Server) -> Result<(), anyhow::Error> {
        let host = self.config.host.as_ref();
        
        if let Some(ssl_config) = &self.config.ssl {
            if ssl_config.enabled {
                // Create HTTPS service with SSL
                self.add_https_service(server, host, ssl_config)?;
                
                // Add ACME challenge service if enabled
                if let Some(acme_config) = &ssl_config.acme {
                    if acme_config.enabled {
                        self.add_acme_service(server, acme_config)?;
                    }
                }
            } else {
                // Create HTTP service (SSL disabled)
                self.add_http_service(server, host)?;
            }
        } else {
            // No SSL configuration, create HTTP service
            self.add_http_service(server, host)?;
        }
        
        Ok(())
    }
    
    fn add_https_service(
        &self,
        server: &mut Server,
        host: &str,
        ssl_config: &crate::config::ProxySsl,
    ) -> Result<(), anyhow::Error> {
        let router = Router {};
        let mut https_service = http_proxy_service(&server.configuration, router);
        
        // Setup TLS settings
        let cert_store = CertStore::new();
        let mut tls_settings = TlsSettings::with_callbacks(Box::new(cert_store))?;
        tls_settings.enable_h2();
        
        // Set protocol versions
        tls_settings.set_min_proto_version(Some((&ssl_config.min_proto).into()))?;
        tls_settings.set_max_proto_version(Some((&ssl_config.max_proto).into()))?;
        
        tls_settings.set_servername_callback(move |ssl_ref, _| CertStore::sni_callback(ssl_ref));
        
        // Add TLS settings to service
        https_service.add_tls_with_settings(host, None, tls_settings);
        
        // Set worker threads
        https_service.threads = self.config.worker_threads;
        
        server.add_service(https_service);
        
        tracing::info!(
            "SSL enabled - HTTPS service configured on {} with TLS termination",
            host
        );
        
        Ok(())
    }
    
    fn add_http_service(&self, server: &mut Server, host: &str) -> Result<(), anyhow::Error> {
        let router = Router {};
        let mut http_service = http_proxy_service(&server.configuration, router);
        
        http_service.add_tcp(host);
        
        // Set worker threads
        http_service.threads = self.config.worker_threads;
        
        server.add_service(http_service);
        
        tracing::info!(
            "HTTP service configured on {} (no TLS termination)",
            host
        );
        
        Ok(())
    }
    
    fn add_acme_service(
        &self,
        server: &mut Server,
        acme_config: &crate::config::ProxyAcme,
    ) -> Result<(), anyhow::Error> {
        let http_lb = HttpLB {};
        let mut acme_service = http_proxy_service(&server.configuration, http_lb);
        
        let acme_address = if let Some(port) = acme_config.challenge_port {
            format!("0.0.0.0:{}", port)
        } else {
            "0.0.0.0:80".to_string()
        };
        
        acme_service.add_tcp(&acme_address);
        acme_service.threads = Some(1); // Minimal threads for ACME
        
        server.add_service(acme_service);
        
        tracing::info!(
            "ACME challenge service configured on {}",
            acme_address
        );
        
        Ok(())
    }
}