//! The HTTP and HTTPS services, listening on the addresses the hosts need

use std::{collections::BTreeSet, net::SocketAddr, sync::Arc};

use async_trait::async_trait;
use pingora::{
    apps::ServerApp,
    listeners::tls::TlsSettings,
    protocols::Stream,
    proxy::{http_proxy, http_proxy_service, HttpProxy},
    server::{configuration::ServerConf, ShutdownWatch},
    services::listening::Service,
};

use crate::tcp::proxy::TcpProxy;

use super::{cert_store::CertStore, https_proxy::Router, listener::Listener};

pub type ProxyService = Service<HttpProxy<Router>>;

/// HTTPS, and the `tcp` blocks of hosts sharing its addresses: once TLS is terminated, the
/// connections of those hosts (by SNI) are forwarded, the rest are HTTPS
pub struct HttpsApp {
    http: Arc<HttpProxy<Router>>,
    tcp: Option<TcpProxy>,
}

#[async_trait]
impl ServerApp for HttpsApp {
    async fn process_new(
        self: &Arc<Self>,
        stream: Stream,
        shutdown: &ShutdownWatch,
    ) -> Option<Stream> {
        if let Some(tcp) = &self.tcp {
            if let Some(target) = tcp.target(&stream) {
                tcp.forward(stream, target).await;
                return None;
            }
        }
        self.http.process_new(stream, shutdown).await
    }

    async fn cleanup(&self) {
        self.http.cleanup().await;
    }
}

/// Plain HTTP: hosts without TLS, ACME challenges and redirects to HTTPS
pub fn http_service(
    configuration: &Arc<ServerConf>,
    addresses: &BTreeSet<String>,
    default_address: SocketAddr,
    worker_threads: Option<usize>,
) -> ProxyService {
    let router = Router {
        listener: Listener::Http,
        default_address,
    };
    let mut service = http_proxy_service(configuration, router);
    service.threads = worker_threads;
    for address in addresses {
        service.add_tcp(address);
    }
    service
}

/// HTTPS: certificates by SNI, HTTP/2, hosts with TLS, and the `tcp` blocks on its addresses
pub fn https_service(
    configuration: &Arc<ServerConf>,
    addresses: &BTreeSet<String>,
    default_address: SocketAddr,
    tcp: Option<TcpProxy>,
    worker_threads: Option<usize>,
) -> Result<Service<HttpsApp>, anyhow::Error> {
    let router = Router {
        listener: Listener::Https,
        default_address,
    };
    let app = HttpsApp {
        http: Arc::new(http_proxy(configuration, router)),
        tcp,
    };
    let mut service = Service::new("Pingora HTTP Proxy Service".to_string(), app);
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
