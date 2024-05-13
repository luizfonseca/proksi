use std::{collections::HashMap, sync::Arc};

use ::pingora::{server::Server, services::background::background_service};
use config::load_proxy_config;
use instant_acme::KeyAuthorization;
use pingora::listeners::TlsSettings;
use pingora_load_balancing::{health_check::TcpHealthCheck, LoadBalancer};
use pingora_proxy::http_proxy_service;

mod config;
mod docker;
mod proxy_server;
mod services;
mod tools;

#[derive(Debug)]
pub struct Storage {
    orders: HashMap<String, (String, String, KeyAuthorization)>,
    certificates: HashMap<String, String>,
}

pub type StorageArc = Arc<tokio::sync::Mutex<Storage>>;

impl Storage {
    pub fn new() -> Self {
        Storage {
            orders: HashMap::new(),
            certificates: HashMap::new(),
        }
    }

    pub fn add_order(
        &mut self,
        identifier: String,
        token: String,
        url: String,
        key_auth: KeyAuthorization,
    ) {
        self.orders.insert(identifier, (token, url, key_auth));
    }

    pub fn add_certificate(&mut self, host: String, certificate: String) {
        self.certificates.insert(host, certificate);
    }

    pub fn get_certificate(&self, host: &str) -> Option<&String> {
        self.certificates.get(host)
    }

    pub fn get_orders(&self) -> &HashMap<String, (String, String, KeyAuthorization)> {
        &self.orders
    }

    pub fn get_order(&self, order: &str) -> Option<&(String, String, KeyAuthorization)> {
        self.orders.get(order)
    }
}

impl Default for Storage {
    fn default() -> Self {
        Self::new()
    }
}

fn create_tracing_subscriber() {
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .init()
}

fn main() {
    create_tracing_subscriber();

    let _proxy_config = load_proxy_config("/etc/proksi/configs");

    let storage = Arc::new(tokio::sync::Mutex::new(Storage::new()));

    let test_hosts = vec![
        "grafana.test.unwraper.com".to_string(),
        "prometheus.test.unwraper.com".to_string(),
        "otel.test.unwrapper.com".to_string(),
        "personal.test.unwrapper.com".to_string(),
    ];

    let certificate_store = proxy_server::cert_store::CertStore::new(storage.clone());

    // Setup tls settings
    let mut tls_settings = TlsSettings::with_callbacks(certificate_store).unwrap();
    tls_settings.enable_h2();
    tls_settings
        .set_max_proto_version(Some(pingora::tls::ssl::SslVersion::TLS1_3))
        .unwrap();

    let mut pingora_server = Server::new(None).unwrap();

    let mut router = proxy_server::https_proxy::Router::new();

    let mut upstreams = LoadBalancer::try_from_iter(["127.0.0.1:3000", "127.0.0.1:8200"]).unwrap();
    let insecure_upstreams = LoadBalancer::try_from_iter(["0.0.0.0:80"]).unwrap();
    // Services
    // Service: Health check
    let hc = TcpHealthCheck::new();
    upstreams.set_health_check(hc);
    let health_check_background_service = background_service("health_check", upstreams);
    // Override
    let upstreams = health_check_background_service.task();

    // Service: Docker
    let client = docker::client::create_client();
    let docker_service = background_service("docker", client);

    let letsencrypt_http = services::letsencrypt::http01::HttpLetsencrypt::new(
        &test_hosts,
        "youremail@example.com",
        storage.clone(),
    );

    let le_service = background_service("letsencrypt", letsencrypt_http);

    for host in test_hosts {
        router.add_route(host, upstreams.clone())
    }

    // Service: Load Balancer
    let mut http_service = http_proxy_service(
        &pingora_server.configuration,
        proxy_server::http_proxy::HttpLB(Arc::new(insecure_upstreams)),
    );
    let mut https_service = http_proxy_service(&pingora_server.configuration, router);
    http_service.add_tcp("0.0.0.0:80");

    https_service.add_tls_with_settings("0.0.0.0:443", None, tls_settings);

    pingora_server.add_service(http_service);
    pingora_server.add_service(https_service);
    pingora_server.add_service(health_check_background_service);
    pingora_server.add_service(docker_service);
    pingora_server.add_service(le_service);

    pingora_server.bootstrap();
    pingora_server.run_forever();
}
