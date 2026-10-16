//! Forwards each connection to an upstream of its block

use std::{
    pin::Pin,
    sync::Arc,
    task::{Context, Poll},
};

use async_trait::async_trait;
use pingora::{
    apps::ServerApp,
    protocols::{Stream, ALPN},
    server::ShutdownWatch,
};
use prometheus::IntCounter;
use tokio::{
    io::{AsyncRead, AsyncWrite, ReadBuf},
    net::TcpStream,
};

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
        let _connection = target.metrics.connection();
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
        // Bytes are counted as they're read, so long connections show up while they're open
        let mut downstream = Counted {
            inner: &mut downstream,
            bytes: &target.metrics.bytes_in,
        };
        let mut upstream = Counted {
            inner: &mut upstream,
            bytes: &target.metrics.bytes_out,
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

/// A stream counting the bytes read from it
struct Counted<'a, S> {
    inner: &'a mut S,
    bytes: &'a IntCounter,
}

impl<S: AsyncRead + Unpin> AsyncRead for Counted<'_, S> {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        let before = buf.filled().len();
        let poll = Pin::new(&mut *self.inner).poll_read(cx, buf);
        if let Poll::Ready(Ok(())) = poll {
            self.bytes
                .inc_by(u64::try_from(buf.filled().len() - before).unwrap_or_default());
        }
        poll
    }
}

impl<S: AsyncWrite + Unpin> AsyncWrite for Counted<'_, S> {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<std::io::Result<usize>> {
        Pin::new(&mut *self.inner).poll_write(cx, buf)
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Pin::new(&mut *self.inner).poll_flush(cx)
    }

    fn poll_shutdown(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<std::io::Result<()>> {
        Pin::new(&mut *self.inner).poll_shutdown(cx)
    }
}
