//! The HTTP and HTTPS services, listening on the addresses the hosts need

use std::{collections::BTreeSet, sync::Arc};

use pingora::{
    listeners::tls::TlsSettings,
    proxy::{http_proxy_service, HttpProxy},
    server::configuration::ServerConf,
    services::listening::Service,
};

use super::{cert_store::CertStore, https_proxy::Router, listener::Listener};

pub type ProxyService = Service<HttpProxy<Router>>;

/// Plain HTTP: hosts without TLS, ACME challenges and redirects to HTTPS
pub fn http_service(
    configuration: &Arc<ServerConf>,
    addresses: &BTreeSet<String>,
    worker_threads: Option<usize>,
) -> ProxyService {
    let router = Router {
        listener: Listener::Http,
    };
    let mut service = http_proxy_service(configuration, router);
    service.threads = worker_threads;
    for address in addresses {
        service.add_tcp(address);
    }
    service
}

/// HTTPS: certificates by SNI, HTTP/2, and hosts with TLS
pub fn https_service(
    configuration: &Arc<ServerConf>,
    addresses: &BTreeSet<String>,
    worker_threads: Option<usize>,
) -> Result<ProxyService, anyhow::Error> {
    let router = Router {
        listener: Listener::Https,
    };
    let mut service = http_proxy_service(configuration, router);
    service.threads = worker_threads;
    for address in addresses {
        service.add_tls_with_settings(address, None, tls_settings()?);
    }
    Ok(service)
}

fn tls_settings() -> Result<TlsSettings, anyhow::Error> {
    let mut tls_settings = TlsSettings::with_callbacks(Box::new(CertStore::new()))?;
    tls_settings.enable_h2();

    // tls_settings.set_session_cache_mode(SslSessionCacheMode::SERVER);
    tls_settings.set_servername_callback(move |ssl_ref, _| CertStore::sni_callback(ssl_ref));

    // For now this is a hardcoded recommendation based on
    // https://developers.cloudflare.com/ssl/reference/protocols/
    // but will be made configurable in the future
    tls_settings.set_min_proto_version(Some(pingora::tls::ssl::SslVersion::TLS1_2))?;
    tls_settings.set_max_proto_version(Some(pingora::tls::ssl::SslVersion::TLS1_3))?;
    Ok(tls_settings)
}
