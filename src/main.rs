use std::sync::Arc;

use ::pingora::{server::Server, services::background::background_service};
use pingora_load_balancing::{health_check::TcpHealthCheck, LoadBalancer};
use pingora_proxy::http_proxy_service;

mod docker;
mod proxy_server;

fn main() {
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

    router.add_route("grafana.dev.localhost".into(), upstreams);

    // Service: Load Balancer
    let mut http_service = http_proxy_service(
        &pingora_server.configuration,
        proxy_server::http_proxy::HttpLB(Arc::new(insecure_upstreams)),
    );
    let mut https_service = http_proxy_service(&pingora_server.configuration, router);
    http_service.add_tcp("0.0.0.0:80");
    // https_service.add_tcp("0.0.0.0:443");
    https_service
        .add_tls(
            "0.0.0.0:443",
            "../test-certs/localhost.crt",
            "../test-certs/localhost.key",
        )
        .unwrap();

    pingora_server.add_service(http_service);
    pingora_server.add_service(https_service);
    pingora_server.add_service(health_check_background_service);
    pingora_server.add_service(docker_service);

    pingora_server.bootstrap();
    pingora_server.run_forever();
}
