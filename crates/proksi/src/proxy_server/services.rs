//! The HTTP and HTTPS services, listening on the addresses the hosts need

use std::{collections::BTreeSet, net::SocketAddr, sync::Arc};

use async_trait::async_trait;
use pingora::{
    apps::ServerApp,
    tls::ssl::{AlpnError, NameType, SslAcceptor, SslAcceptorBuilder, SslMethod},
    listeners::tls::TlsSettings,
    protocols::Stream,
    proxy::{http_proxy, HttpProxy},
    server::{configuration::ServerConf, ShutdownWatch},
    services::listening::Service,
};

use crate::tcp::{
    alpn::{select_protocols, AlpnProtocols},
    on_address,
    proxy::TcpProxy,
    TcpTarget,
};

use super::{
    cert_store::CertStore, connections::Counted, https_proxy::Router, listener::Listener,
};

pub type ProxyService = Service<Counted<HttpProxy<Router>>>;

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
                tcp.forward(stream, target, None).await;
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
    let mut service = Service::new(
        "Pingora HTTP Proxy Service".to_string(),
        Counted::new(http_proxy(configuration, router)),
    );
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
    tcp_targets: &Arc<Vec<TcpTarget>>,
    worker_threads: Option<usize>,
) -> Result<Service<Counted<HttpsApp>>, anyhow::Error> {
    let app = https_app(configuration, addresses, default_address, tcp_targets);
    let mut service = Service::new("Pingora HTTP Proxy Service".to_string(), Counted::new(app));
    service.threads = worker_threads;
    for address in addresses {
        let mut settings = tls_settings()?;
        let protocols = AlpnProtocols::new(on_address(tcp_targets, address));
        select_protocols(&mut settings, protocols, true);
        service.add_tls_with_settings(address, None, settings);
    }
    Ok(service)
}

/// The HTTPS app (after TLS is terminated) for `addresses`, with their `tcp` blocks
pub fn https_app(
    configuration: &Arc<ServerConf>,
    addresses: &BTreeSet<String>,
    default_address: SocketAddr,
    tcp_targets: &Arc<Vec<TcpTarget>>,
) -> HttpsApp {
    // `tcp` blocks of hosts sharing these addresses (chosen by SNI, and ALPN)
    let tcp = addresses
        .iter()
        .any(|address| on_address(tcp_targets, address).next().is_some())
        .then(|| TcpProxy::new(tcp_targets.clone()));

    let router = Router {
        listener: Listener::Https,
        default_address,
    };
    HttpsApp {
        http: Arc::new(http_proxy(configuration, router)),
        tcp,
    }
}

fn tls_settings() -> Result<TlsSettings, anyhow::Error> {
    let mut tls_settings = TlsSettings::with_callbacks(Box::new(CertStore::new()))?;
    tls_settings.enable_h2();
    configure_tls(&mut tls_settings)?;
    Ok(tls_settings)
}

/// TLS terminated by proksi itself (after reading the `ClientHello`, see `tcp::router`), set up
/// like the HTTPS listener: certificates by SNI, then HTTP/2, HTTP/1.1 and the `tcp` blocks'
/// protocols
pub fn tls_acceptor(protocols: AlpnProtocols) -> Result<SslAcceptor, anyhow::Error> {
    let mut builder = SslAcceptor::mozilla_intermediate_v5(SslMethod::tls())?;
    configure_tls(&mut builder)?;
    builder.set_alpn_select_callback(move |ssl, offered| {
        protocols
            .select(ssl.servername(NameType::HOST_NAME), offered, true)
            .ok_or(AlpnError::NOACK)
    });
    Ok(builder.build())
}

fn configure_tls(builder: &mut SslAcceptorBuilder) -> Result<(), anyhow::Error> {
    // tls_settings.set_session_cache_mode(SslSessionCacheMode::SERVER);
    builder.set_servername_callback(move |ssl_ref, _| CertStore::sni_callback(ssl_ref));

    // For now this is a hardcoded recommendation based on
    // https://developers.cloudflare.com/ssl/reference/protocols/
    // but will be made configurable in the future
    builder.set_min_proto_version(Some(pingora::tls::ssl::SslVersion::TLS1_2))?;
    builder.set_max_proto_version(Some(pingora::tls::ssl::SslVersion::TLS1_3))?;
    Ok(())
}
