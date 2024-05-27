use std::{borrow::Cow, sync::Arc};

use ::pingora::server::Server;
use anyhow::anyhow;
use config::load;
use dashmap::DashMap;

use once_cell::sync::Lazy;
use pingora::listeners::TlsSettings;
use pingora_proxy::http_proxy_service;
use proxy_server::cert_store;
use services::{
    discovery::RoutingService,
    docker::{self},
    health_check::HealthService,
    letsencrypt::http01::LetsencryptService,
    logger::{ProxyLog, ProxyLoggerReceiver},
};
use stores::{certificates::CertificateStore, routes::RouteStore};

mod channel;
mod config;
mod proxy_server;
mod services;
mod stores;
mod tools;

/// Static reference to the route store that can be shared across threads
pub static ROUTE_STORE: Lazy<RouteStore> = Lazy::new(|| Arc::new(DashMap::new()));

/// Static reference to the certificate store that can be shared across threads
pub static CERT_STORE: Lazy<CertificateStore> = Lazy::new(|| Arc::new(DashMap::new()));

#[derive(Clone)]
pub struct MsgRoute {
    host: Cow<'static, str>,
    upstreams: Vec<String>,
}
#[derive(Clone)]
pub struct MsgCert {
    _cert: Vec<u8>,
    _key: Vec<u8>,
}

#[derive(Clone)]
pub enum MsgProxy {
    NewRoute(MsgRoute),
    NewCertificate(MsgCert),
}

fn main() -> Result<(), anyhow::Error> {
    // Loads configuration from command-line, YAML or TOML sources
    let proxy_config = Arc::new(
        load("/etc/proksi/configs").map_err(|e| anyhow!("Failed to load configuration: {}", e))?,
    );

    // Receiver channel for Routes/Certificates/etc
    let (sender, mut _receiver) = tokio::sync::broadcast::channel::<MsgProxy>(100);

    // Receiver channel for non-blocking logging
    let (log_sender, log_receiver) = tokio::sync::mpsc::unbounded_channel::<Vec<u8>>();
    let proxy_logger = ProxyLog::new(&log_sender);

    // Creates a tracing/logging subscriber based on the configuration provided
    tracing_subscriber::fmt()
        .with_max_level(&proxy_config.logging.level)
        .compact()
        .with_writer(proxy_logger)
        .init();

    // Pingora load balancer server
    let mut pingora_server = Server::new(None)?;

    let certificate_store = Box::new(cert_store::CertStore::new());

    // Setup tls settings and Enable HTTP/2
    let mut tls_settings = TlsSettings::with_callbacks(certificate_store).unwrap();
    tls_settings.enable_h2();
    tls_settings.set_max_proto_version(Some(pingora::tls::ssl::SslVersion::TLS1_3))?;

    // Service: Docker
    if proxy_config.docker.enabled.unwrap_or(false) {
        let docker_service = docker::LabelService::new(proxy_config.clone(), sender.clone());
        pingora_server.add_service(docker_service);
    }

    // Service: Lets Encrypt HTTP Challenge/Certificate renewal
    let challenge_store = Arc::new(DashMap::<String, (String, String)>::new());
    if proxy_config.lets_encrypt.enabled.unwrap_or(false) {
        let letsencrypt_service =
            LetsencryptService::new(proxy_config.clone(), challenge_store.clone());
        pingora_server.add_service(letsencrypt_service);
    }

    // Service: HTTP Load Balancer (only used by acme-challenges)
    // As we don't necessarily need an upstream to handle the acme-challenges,
    // we can use a simple mock LoadBalancer
    let mut http_public_service = http_proxy_service(
        &pingora_server.configuration,
        proxy_server::http_proxy::HttpLB { challenge_store },
    );

    // Service: HTTPS Load Balancer (main service)
    // The router will also handle health checks and failover in case of upstream failure
    let router = proxy_server::https_proxy::Router {};
    let mut https_secure_service = http_proxy_service(&pingora_server.configuration, router);
    http_public_service.add_tcp("0.0.0.0:80");

    // Worker threads per configuration
    https_secure_service.threads = proxy_config.worker_threads;
    https_secure_service.add_tls_with_settings("0.0.0.0:443", None, tls_settings);

    pingora_server.add_service(RoutingService::new(proxy_config.clone(), sender.clone()));
    pingora_server.add_service(HealthService::new());
    pingora_server.add_service(ProxyLoggerReceiver::new(log_receiver));

    // Listen on HTTP and HTTPS ports
    pingora_server.add_service(http_public_service);
    pingora_server.add_service(https_secure_service);

    pingora_server.bootstrap();
    pingora_server.run_forever();
}
