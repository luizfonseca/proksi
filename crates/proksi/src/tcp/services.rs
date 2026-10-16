//! The services accepting TCP connections, and the health checks of their upstreams

use std::{collections::BTreeSet, net::SocketAddr, sync::Arc};

use pingora::{
    lb::health_check::TcpHealthCheck,
    listeners::tls::TlsSettings,
    services::{listening::Service, ServiceWithDependents},
};

use crate::{
    config::tcp::{TcpRoute, TcpTls},
    proxy_server::cert_store::CertStore,
    routing::build::load_balancer,
};

use super::{
    alpn::{select_protocols, AlpnProtocols},
    on_address,
    proxy::TcpProxy,
    register_load_balancers, TcpTarget,
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
        });
    }
    register_load_balancers(&targets);
    Ok(Arc::new(targets))
}

/// Services for the `tcp` blocks: plain TCP (and TLS passthrough), and TLS terminated with the
/// hosts' certificates. Addresses shared with HTTPS hosts are served by the HTTPS service.
pub fn tcp_services(
    routes: &[TcpRoute],
    targets: &Arc<Vec<TcpTarget>>,
    https: &BTreeSet<String>,
    worker_threads: Option<usize>,
) -> Result<Vec<Box<dyn ServiceWithDependents>>, anyhow::Error> {
    let mut services: Vec<Box<dyn ServiceWithDependents>> = vec![];

    let addresses = |tls: bool| {
        routes
            .iter()
            .filter(|route| (route.tls == TcpTls::Terminate) == tls)
            .filter(|route| !https.contains(&route.listen))
            .map(|route| route.listen.clone())
            .collect::<BTreeSet<_>>()
    };
    let (plain, with_tls) = (addresses(false), addresses(true));

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
