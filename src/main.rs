use std::sync::Arc;

use ::pingora::{server::Server, services::background::background_service};
use arc_swap::ArcSwap;
use config::load_proxy_config;

use once_cell::sync::Lazy;
use pingora::listeners::TlsSettings;
use pingora_load_balancing::{health_check::TcpHealthCheck, LoadBalancer};
use pingora_proxy::http_proxy_service;
use services::logger::{ProxyLogger, ProxyLoggerReceiver};
use stores::{certificates::CertificatesStore, routes::RouteStore};
use tokio::sync::mpsc;

mod config;
mod docker;
mod proxy_server;
mod services;
mod stores;
mod tools;

/// Static reference to the route store that can be shared across threads
pub static ROUTE_STORE: Lazy<ArcSwap<RouteStore>> =
    Lazy::new(|| ArcSwap::from_pointee(RouteStore::new()));

pub static CERT_STORE: Lazy<ArcSwap<CertificatesStore>> =
    Lazy::new(|| ArcSwap::from_pointee(CertificatesStore::new()));

fn main() -> Result<(), anyhow::Error> {
    // Loads configuration from command-line, YAML or TOML sources
    let proxy_config = load_proxy_config("/etc/proksi/configs")?;

    // let file_appender = tracing_appender::rolling::hourly("./tmp", "proksi.log");
    let (log_sender, log_receiver) = mpsc::unbounded_channel::<Vec<u8>>();
    let proxy_logger = ProxyLogger::new(log_sender);

    // Creates a tracing/logging subscriber based on the configuration provided
    tracing_subscriber::fmt()
        .with_max_level(&proxy_config.logging.level)
        .compact()
        .with_writer(proxy_logger)
        .init();

    // Pingora load balancer server
    let mut pingora_server = Server::new(None)?;

    // Request router:
    // Given a host header, the router will return the corresponding upstreams
    let mut router_store = RouteStore::new();

    // for each route, build a loadbalancer configuration with the corresponding upstreams
    for route in proxy_config.routes {
        // Construct host:port SocketAddr strings for each upstream
        let addr_upstreams = route
            .upstreams
            .iter()
            .map(|upstr| format!("{}:{}", upstr.ip, upstr.port));

        let mut upstreams = LoadBalancer::try_from_iter(addr_upstreams)?;
        let tcp_health_check = TcpHealthCheck::new();
        upstreams.set_health_check(tcp_health_check);

        let health_check_service = background_service(&route.host, upstreams);
        let upstreams = health_check_service.task();

        router_store.add_route(route.host, upstreams);
        pingora_server.add_service(health_check_service);
    }

    let certificate_store = proxy_server::cert_store::CertStore::new();

    // Setup tls settings and Enable HTTP/2
    let mut tls_settings = TlsSettings::with_callbacks(certificate_store).unwrap();
    tls_settings.enable_h2();
    tls_settings.set_max_proto_version(Some(pingora::tls::ssl::SslVersion::TLS1_3))?;

    // Service: Docker
    let client = docker::client::create_client();
    let docker_service = background_service("docker", client);

    // Service: Lets Encrypt HTTP Challenge/Certificate renewal
    let letsencrypt_http = services::letsencrypt::http01::HttpLetsencrypt::new(
        &router_store.get_route_keys(),
        "youremail@example.com",
    );
    let le_service = background_service("letsencrypt", letsencrypt_http);

    // Service: HTTP Load Balancer (only used by acme-challenges)
    // As we don't necessarily need an upstream to handle the acme-challenges,
    // we can use a simple mock LoadBalancer
    let mut http_service = http_proxy_service(
        &pingora_server.configuration,
        proxy_server::http_proxy::HttpLB(Arc::new(
            LoadBalancer::try_from_iter(["127.0.0.1:80"]).unwrap(),
        )),
    );

    // Service: HTTPS Load Balancer (main service)
    // The router will also handle health checks and failover in case of upstream failure
    ROUTE_STORE.swap(Arc::new(router_store));
    let router = proxy_server::https_proxy::Router {};
    let mut https_service = http_proxy_service(&pingora_server.configuration, router);
    http_service.add_tcp("0.0.0.0:80");

    // Worker threads per configuration
    https_service.threads = proxy_config.worker_threads;
    https_service.add_tls_with_settings("0.0.0.0:443", None, tls_settings);

    pingora_server.add_service(http_service);
    pingora_server.add_service(https_service);
    pingora_server.add_service(docker_service);
    pingora_server.add_service(le_service);
    pingora_server.add_service(ProxyLoggerReceiver(log_receiver));

    pingora_server.bootstrap();
    pingora_server.run_forever();
}
