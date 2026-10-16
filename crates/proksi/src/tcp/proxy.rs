//! Forwards each connection to an upstream of its block

use std::sync::Arc;

use async_trait::async_trait;
use pingora::{
    apps::ServerApp,
    protocols::{Stream, ALPN},
    server::ShutdownWatch,
};
use tokio::net::TcpStream;

use crate::proxy_server::cert_store::ServerName;

use super::{choose, TcpTarget};

pub struct TcpProxy {
    targets: Arc<Vec<TcpTarget>>,
}

impl TcpProxy {
    pub fn new(targets: Arc<Vec<TcpTarget>>) -> Self {
        Self { targets }
    }

    /// The block of a connection, by the address it was accepted on and, with TLS terminated,
    /// the name the client asked for
    pub fn target(&self, downstream: &Stream) -> Option<&TcpTarget> {
        let local = downstream
            .get_socket_digest()
            .and_then(|digest| digest.local_addr().and_then(|addr| addr.as_inet()).copied())?;
        // Set once TLS is terminated (see `CertStore`)
        let sni = downstream.get_ssl_digest().and_then(|digest| {
            digest
                .extension
                .get::<ServerName>()
                .map(|name| name.0.clone())
        });

        let alpn = match downstream.selected_alpn_proto() {
            Some(ALPN::Custom(protocol)) => Some(protocol),
            _ => None,
        };

        let target = choose(
            &self.targets,
            &local,
            sni.as_deref(),
            alpn.as_ref().map(pingora::protocols::tls::CustomALPN::protocol),
        );
        if target.is_none() {
            tracing::debug!("no tcp block for a connection on {local} (server name: {sni:?})");
        }
        target
    }

    /// Forwards the connection to an upstream of `target`, until either side closes it
    pub async fn forward(&self, mut downstream: Stream, target: &TcpTarget) {
        let Some(addr) = target
            .load_balancer
            .select(b"", 32)
            .and_then(|backend| backend.addr.as_inet().copied())
        else {
            tracing::warn!("{}: no healthy upstream", target.label);
            return;
        };

        let mut upstream = match TcpStream::connect(addr).await {
            Ok(upstream) => upstream,
            Err(err) => {
                tracing::warn!(
                    "{}: could not connect to upstream {addr}: {err}",
                    target.label
                );
                return;
            }
        };
        if let Err(err) = tokio::io::copy_bidirectional(&mut downstream, &mut upstream).await {
            tracing::debug!("{}: connection to {addr} ended: {err}", target.label);
        }
    }
}

#[async_trait]
impl ServerApp for TcpProxy {
    async fn process_new(
        self: &Arc<Self>,
        downstream: Stream,
        _shutdown: &ShutdownWatch,
    ) -> Option<Stream> {
        let target = self.target(&downstream)?;
        self.forward(downstream, target).await;
        None
    }
}
