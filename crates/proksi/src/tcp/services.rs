//! The services accepting TCP connections, and the health checks of their upstreams

use std::{collections::BTreeSet, net::SocketAddr, sync::Arc};

use pingora::{
    lb::health_check::TcpHealthCheck,
    listeners::tls::TlsSettings,
    services::{listening::Service, ServiceWithDependents},
};

use pingora::server::configuration::ServerConf;

use crate::{
    config::tcp::{TcpRoute, TcpTls},
    proxy_server::services::{https_app, tls_acceptor},
    proxy_server::cert_store::CertStore,
    metrics::TcpMetrics,
    routing::build::load_balancer,
};

use super::{
    alpn::{select_protocols, AlpnProtocols},
    on_address,
    proxy::TcpProxy,
    register_load_balancers,
    router::SniRouter,
    TcpTarget,
};

/// The `tcp` blocks, ready to accept connections (their upstreams are health checked with
/// the ones of the locations, see `services::health_check`)
pub fn tcp_targets(routes: &[TcpRoute]) -> Result<Arc<Vec<TcpTarget>>, anyhow::Error> {
    let mut targets = vec![];
    for route in routes {
        let mut load_balancer = load_balancer(&route.upstreams)?;
        load_balancer.set_health_check(TcpHealthCheck::new());
        targets.push(TcpTarget {
            label: route.label(),
            host: route.host.clone(),
            listen: route.listen.parse::<SocketAddr>()?,
            tls: route.tls,
            alpn: route.alpn.clone(),
            load_balancer: Arc::new(load_balancer),
            metrics: crate::metrics::enabled().then(|| TcpMetrics::new(&route.label())),
        });
    }
    register_load_balancers(&targets);
    Ok(Arc::new(targets))
}

/// Addresses with TLS passthrough blocks, served by `tcp::router`
pub fn passthrough_addresses(routes: &[TcpRoute]) -> BTreeSet<String> {
    routes
        .iter()
        .filter(|route| route.tls == TcpTls::Passthrough)
        .map(|route| route.listen.clone())
        .collect()
}

/// Services for the `tcp` blocks: plain TCP, TLS terminated with the hosts' certificates, and
/// addresses with TLS passthrough (with the HTTPS hosts sharing them). Other addresses shared
/// with HTTPS hosts are served by the HTTPS service.
pub fn tcp_services(
    routes: &[TcpRoute],
    targets: &Arc<Vec<TcpTarget>>,
    https: &BTreeSet<String>,
    (configuration, default_https): (&Arc<ServerConf>, SocketAddr),
    worker_threads: Option<usize>,
) -> Result<Vec<Box<dyn ServiceWithDependents>>, anyhow::Error> {
    let mut services: Vec<Box<dyn ServiceWithDependents>> = vec![];

    let passthrough = passthrough_addresses(routes);
    let addresses = |tls: TcpTls| {
        routes
            .iter()
            .filter(|route| route.tls == tls)
            .filter(|route| !https.contains(&route.listen) && !passthrough.contains(&route.listen))
            .map(|route| route.listen.clone())
            .collect::<BTreeSet<_>>()
    };
    let (plain, with_tls) = (addresses(TcpTls::None), addresses(TcpTls::Terminate));

    // The server name is read before the handshake: forwarded untouched, or terminated
    for address in &passthrough {
        let shared = https.contains(address).then(|| {
            let addresses = BTreeSet::from([address.clone()]);
            let app = https_app(configuration, &addresses, default_https, targets);
            let protocols = AlpnProtocols::new(on_address(targets, address));
            tls_acceptor(protocols).map(|acceptor| (acceptor, Arc::new(app)))
        });
        let sni_router = SniRouter::new(TcpProxy::new(targets.clone()), shared.transpose()?);
        let mut service = Service::new(format!("tls passthrough on {address}"), sni_router);
        service.threads = worker_threads;
        service.add_tcp(address);
        services.push(Box::new(service));
    }

    if !plain.is_empty() {
        let mut service = Service::new("tcp".to_string(), TcpProxy::new(targets.clone()));
        service.threads = worker_threads;
        for address in &plain {
            service.add_tcp(address);
        }
        services.push(Box::new(service));
    }
    if !with_tls.is_empty() {
        let mut service = Service::new("tcp with tls".to_string(), TcpProxy::new(targets.clone()));
        service.threads = worker_threads;
        for address in &with_tls {
            let mut settings = tls_settings()?;
            let protocols = AlpnProtocols::new(on_address(targets, address));
            select_protocols(&mut settings, protocols, false);
            service.add_tls_with_settings(address, None, settings);
        }
        services.push(Box::new(service));
    }

    Ok(services)
}

/// The hosts' certificates (by SNI), as on the HTTPS port, without HTTP/2
fn tls_settings() -> Result<TlsSettings, anyhow::Error> {
    let mut settings = TlsSettings::with_callbacks(Box::new(CertStore::new()))?;
    settings.set_servername_callback(move |ssl_ref, _| CertStore::sni_callback(ssl_ref));
    settings.set_min_proto_version(Some(pingora::tls::ssl::SslVersion::TLS1_2))?;
    settings.set_max_proto_version(Some(pingora::tls::ssl::SslVersion::TLS1_3))?;
    Ok(settings)
}
