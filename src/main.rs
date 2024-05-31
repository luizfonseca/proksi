use std::{borrow::Cow, sync::Arc};

use ::pingora::server::Server;
use anyhow::anyhow;
use bytes::Bytes;
use config::load;
use dashmap::DashMap;

use pingora::{listeners::TlsSettings, server::configuration::Opt};
use pingora_proxy::http_proxy_service;
use proxy_server::cert_store::CertStore;
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

#[derive(Clone, Default)]
pub struct MsgRoute {
    host: Cow<'static, str>,
    upstreams: Vec<String>,
    path_matchers: Vec<String>,
}

#[derive(Clone)]
pub struct MsgCert {
    _cert: Bytes,
    _key: Bytes,
}

#[derive(Clone)]
pub enum MsgProxy {
    NewRoute(MsgRoute),
    NewCertificate(MsgCert),
}

fn main() -> Result<(), anyhow::Error> {
    // Stores (Routes, Certificates, Challenges)
    let route_store: RouteStore = Arc::new(DashMap::new());
    let certificate_store: CertificateStore = Arc::new(DashMap::new());
    let challenge_store = Arc::new(DashMap::<String, (String, String)>::new());

    // Loads configuration from command-line, YAML or TOML sources
    let proxy_config = Arc::new(
        load("/etc/proksi/configs").map_err(|e| anyhow!("Failed to load configuration: {}", e))?,
    );

    // Receiver channel for Routes/Certificates/etc
    let (sender, mut _receiver) = tokio::sync::broadcast::channel::<MsgProxy>(10);

    // Receiver channel for non-blocking logging
    let (log_sender, log_receiver) = tokio::sync::mpsc::unbounded_channel::<bytes::Bytes>();
    let proxy_logger = ProxyLog::new(&log_sender);

    // Creates a tracing/logging subscriber based on the configuration provided
    tracing_subscriber::fmt()
        .with_max_level(&proxy_config.logging.level)
        .compact()
        .with_writer(proxy_logger)
        .init();

    // Pingora load balancer server
    let mut pingora_server = Server::new(Some(Opt::default()))?;

    // Service: Docker
    if proxy_config.docker.enabled.unwrap_or(false) {
        let docker_service = docker::LabelService::new(proxy_config.clone(), sender.clone());
        pingora_server.add_service(docker_service);
    }

    // Service: Lets Encrypt HTTP Challenge/Certificate renewal
    if proxy_config.lets_encrypt.enabled.unwrap_or(false) {
        let letsencrypt_service = LetsencryptService {
            config: proxy_config.clone(),
            challenge_store: challenge_store.clone(),
            cert_store: certificate_store.clone(),
            route_store: route_store.clone(),
        };
        pingora_server.add_service(letsencrypt_service);
    }

    // Service: HTTP Load Balancer (only used by acme-challenges)
    // As we don't necessarily need an upstream to handle the acme-challenges,
    // we can use a simple mock LoadBalancer
    let mut http_public_service = http_proxy_service(
        &pingora_server.configuration,
        proxy_server::http_proxy::HttpLB {
            challenge_store: challenge_store.clone(),
        },
    );

    // Service: HTTPS Load Balancer (main service)
    // The router will also handle health checks and failover in case of upstream failure
    let router = proxy_server::https_proxy::Router {
        store: route_store.clone(),
    };
    let mut https_secure_service = http_proxy_service(&pingora_server.configuration, router);
    http_public_service.add_tcp("0.0.0.0:80");

    // Worker threads per configuration
    https_secure_service.threads = proxy_config.worker_threads;

    // Setup tls settings and Enable HTTP/2
    let cert_store = CertStore::new(certificate_store.clone());
    let mut tls_settings = TlsSettings::with_callbacks(Box::new(cert_store)).unwrap();
    tls_settings.enable_h2();
    tls_settings.set_servername_callback(move |ssl_ref, _| {
        CertStore::sni_callback(ssl_ref, &certificate_store)
    });

    // For now this is a hardcoded recommendation based on
    // https://developers.cloudflare.com/ssl/reference/protocols/
    // but will be made configurable in the future
    tls_settings.set_min_proto_version(Some(pingora::tls::ssl::SslVersion::TLS1_2))?;
    tls_settings.set_max_proto_version(Some(pingora::tls::ssl::SslVersion::TLS1_3))?;

    // Add TLS settings to the HTTPS service
    https_secure_service.add_tls_with_settings("0.0.0.0:443", None, tls_settings);

    // Built-in services for health checks, logging, and routing
    pingora_server.add_service(RoutingService::new(
        proxy_config.clone(),
        sender.clone(),
        route_store.clone(),
    ));
    pingora_server.add_service(HealthService::new(route_store.clone()));
    pingora_server.add_service(ProxyLoggerReceiver::new(log_receiver));

    // Listen on HTTP and HTTPS ports
    pingora_server.add_service(http_public_service);
    pingora_server.add_service(https_secure_service);

    pingora_server.bootstrap();
    pingora_server.run_forever();
}
