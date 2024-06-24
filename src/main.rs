use std::{borrow::Cow, sync::Arc};

use ::pingora::server::Server;
use anyhow::anyhow;
use bytes::Bytes;
use clap::crate_version;
use config::{load, Config, LogFormat, RouteHeaderAdd, RouteHeaderRemove, RoutePlugin};
// use openssl::ssl::SslSessionCacheMode;

use pingora::{listeners::TlsSettings, proxy::http_proxy_service, server::configuration::Opt};

use proxy_server::cert_store::CertStore;
use services::{
    discovery::RoutingService,
    docker::{self},
    health_check::HealthService,
    letsencrypt::http01::LetsencryptService,
};
use tracing_appender::non_blocking::{NonBlocking, WorkerGuard};

mod cache;
mod channel;
mod config;
mod plugins;
mod proxy_server;
mod services;
mod stores;
mod tools;

#[derive(Clone, Default)]
pub struct MsgRoute {
    host: Cow<'static, str>,
    upstreams: Vec<String>,
    path_matchers: Vec<String>,
    host_headers_add: Vec<RouteHeaderAdd>,
    host_headers_remove: Vec<RouteHeaderRemove>,
    plugins: Vec<RoutePlugin>,

    self_signed_certs: bool,
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

fn get_non_blocking_writer(config: &Config) -> (NonBlocking, WorkerGuard) {
    // If a path is provided, create a file appender
    if let Some(path) = config.logging.path.clone() {
        let appender = match config.logging.rotation {
            config::LogRotation::Daily => tracing_appender::rolling::daily,
            config::LogRotation::Hourly => tracing_appender::rolling::hourly,
            config::LogRotation::Minutely => tracing_appender::rolling::minutely,
            config::LogRotation::Never => tracing_appender::rolling::never,
        };

        return tracing_appender::non_blocking(appender(path, "proksi"));
    }

    // otherwise, create a stdout appender (default)
    tracing_appender::non_blocking(std::io::stdout())
}

#[deny(
    clippy::all,
    clippy::pedantic,
    clippy::perf,
    clippy::correctness,
    clippy::style,
    clippy::suspicious,
    clippy::complexity
)]
fn main() -> Result<(), anyhow::Error> {
    // Loads configuration from command-line, YAML or TOML sources
    let proxy_config = Arc::new(
        load("/etc/proksi/configs").map_err(|e| anyhow!("Failed to load configuration: {}", e))?,
    );

    // Receiver channel for Routes/Certificates/etc
    let (sender, mut _receiver) = tokio::sync::broadcast::channel::<MsgProxy>(10);

    let (appender, _guard) = get_non_blocking_writer(&proxy_config);

    // Creates a tracing/logging subscriber based on the configuration provided
    if proxy_config.logging.format == LogFormat::Json {
        tracing_subscriber::fmt()
            .json()
            .with_max_level(&proxy_config.logging.level)
            .with_writer(appender)
            .init();
    } else {
        tracing_subscriber::fmt()
            .with_max_level(&proxy_config.logging.level)
            .with_writer(appender)
            .init();
    };

    // Pingora load balancer server
    let pingora_opts = Opt {
        daemon: proxy_config.daemon,
        ..Default::default()
    };

    let mut pingora_server = Server::new(Some(pingora_opts))?;
    pingora_server.bootstrap();

    // Built-in services for health checks, logging, and routing
    pingora_server.add_service(RoutingService::new(
        proxy_config.clone(),
        sender.clone(),
        // route_store.clone(),
    ));

    // Service: Docker
    if proxy_config.docker.enabled.unwrap_or(false) {
        let docker_service = docker::LabelService::new(proxy_config.clone(), sender.clone());
        pingora_server.add_service(docker_service);
    }

    // Service: Lets Encrypt HTTP Challenge/Certificate renewal
    if proxy_config.lets_encrypt.enabled.unwrap_or(false) {
        let letsencrypt_service = LetsencryptService {
            config: proxy_config.clone(),
        };
        pingora_server.add_service(letsencrypt_service);
    }

    // Service: HTTP Load Balancer (only used by acme-challenges)
    // As we don't necessarily need an upstream to handle the acme-challenges,
    // we can use a simple mock LoadBalancer
    let mut http_public_service = http_proxy_service(
        &pingora_server.configuration,
        proxy_server::http_proxy::HttpLB {},
    );

    // Service: HTTPS Load Balancer (main service)
    // The router will also handle health checks and failover in case of upstream failure
    let router = proxy_server::https_proxy::Router {};
    let mut https_secure_service = http_proxy_service(&pingora_server.configuration, router);
    http_public_service.add_tcp("0.0.0.0:80");

    // Worker threads per configuration
    https_secure_service.threads = proxy_config.worker_threads;

    // Setup tls settings and Enable HTTP/2
    let cert_store = CertStore::new();
    let mut tls_settings = TlsSettings::with_callbacks(Box::new(cert_store)).unwrap();
    tls_settings.enable_h2();

    // tls_settings.set_session_cache_mode(SslSessionCacheMode::SERVER);
    tls_settings.set_servername_callback(move |ssl_ref, _| CertStore::sni_callback(ssl_ref));

    // For now this is a hardcoded recommendation based on
    // https://developers.cloudflare.com/ssl/reference/protocols/
    // but will be made configurable in the future
    tls_settings.set_min_proto_version(Some(pingora::tls::ssl::SslVersion::TLS1_2))?;
    tls_settings.set_max_proto_version(Some(pingora::tls::ssl::SslVersion::TLS1_3))?;

    // Add TLS settings to the HTTPS service
    https_secure_service.add_tls_with_settings("0.0.0.0:443", None, tls_settings);

    // Add Prometheus service
    // let mut prometheus_service_http = Service::prometheus_http_service();
    // prometheus_service_http.add_tcp("0.0.0.0:9090");
    // pingora_server.add_service(prometheus_service_http);

    pingora_server.add_service(HealthService::new());

    // Listen on HTTP and HTTPS ports
    pingora_server.add_service(http_public_service);
    pingora_server.add_service(https_secure_service);

    tracing::info!(
        version = crate_version!(),
        workers = proxy_config.worker_threads,
        "running on :443 and :80"
    );
    pingora_server.run_forever();
}
