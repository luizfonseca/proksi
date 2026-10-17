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

use crate::{metrics::TcpMetrics, proxy_server::cert_store::ServerName};

use super::{choose, TcpTarget};

pub struct TcpProxy {
    targets: Arc<Vec<TcpTarget>>,
}

impl TcpProxy {
    pub fn new(targets: Arc<Vec<TcpTarget>>) -> Self {
        Self { targets }
    }

    pub fn targets(&self) -> &[TcpTarget] {
        &self.targets
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

    /// Forwards the connection to an upstream of `target`, until either side closes it, then
    /// writes its access log. `server_name`: the name the client asked for, when TLS isn't
    /// terminated (passthrough)
    pub async fn forward(
        &self,
        mut downstream: Stream,
        target: &TcpTarget,
        server_name: Option<String>,
    ) {
        let _connection = target.metrics.as_ref().map(TcpMetrics::connection);
        let start = std::time::Instant::now();
        let client_ip = downstream
            .get_socket_digest()
            .and_then(|digest| digest.peer_addr().map(ToString::to_string));
        let server_name = server_name.or_else(|| {
            downstream.get_ssl_digest().and_then(|digest| {
                digest
                    .extension
                    .get::<ServerName>()
                    .map(|name| name.0.clone())
            })
        });

        let mut forwarded = Forwarded::default();
        let error = Self::copy(&mut downstream, target, &mut forwarded).await.err();

        tracing::info!(
            protocol = "tcp",
            block = target.label,
            client_ip,
            server_name,
            upstream = forwarded.upstream.map(|addr| addr.to_string()),
            duration_ms = u64::try_from(start.elapsed().as_millis()).unwrap_or(u64::MAX),
            bytes_in = forwarded.bytes_in,
            bytes_out = forwarded.bytes_out,
            error,
            access_log = true
        );
    }

    /// Connects to an upstream and copies both ways, until either side closes the connection
    async fn copy(
        downstream: &mut Stream,
        target: &TcpTarget,
        forwarded: &mut Forwarded,
    ) -> Result<(), String> {
        let addr = target
            .load_balancer
            .select(b"", 32)
            .and_then(|backend| backend.addr.as_inet().copied())
            .ok_or_else(|| {
                tracing::warn!("{}: no healthy upstream", target.label);
                // A name without addresses may resolve again (ADR 0006)
                let load_balancer = target.load_balancer.clone();
                tokio::spawn(async move { load_balancer.update().await });
                "no healthy upstream".to_string()
            })?;
        forwarded.upstream = Some(addr);

        let connect = tokio::time::timeout(target.connect_timeout, TcpStream::connect(addr)).await;
        let mut upstream = match connect {
            Ok(Ok(upstream)) => upstream,
            failed => {
                let err = match failed {
                    Ok(Err(err)) => err.to_string(),
                    _ => format!("no answer within {:?}", target.connect_timeout),
                };
                tracing::warn!("{}: could not connect to upstream {addr}: {err}", target.label);
                // The upstream's name may point elsewhere now (ADR 0006)
                let load_balancer = target.load_balancer.clone();
                tokio::spawn(async move { load_balancer.update().await });
                return Err(format!("could not connect to the upstream: {err}"));
            }
        };

        // Bytes are counted as they're read, so long connections show up while they're open
        let mut downstream = Counted {
            inner: downstream,
            bytes: target.metrics.as_ref().map(|metrics| &metrics.bytes_in),
            read: &mut forwarded.bytes_in,
        };
        let mut upstream = Counted {
            inner: &mut upstream,
            bytes: target.metrics.as_ref().map(|metrics| &metrics.bytes_out),
            read: &mut forwarded.bytes_out,
        };
        tokio::io::copy_bidirectional(&mut downstream, &mut upstream)
            .await
            .map(|_| ())
            .map_err(|err| {
                tracing::debug!("{}: connection to {addr} ended: {err}", target.label);
                err.to_string()
            })
    }
}

/// What a connection forwarded, for its access log
#[derive(Default)]
struct Forwarded {
    upstream: Option<std::net::SocketAddr>,
    bytes_in: u64,
    bytes_out: u64,
}

#[async_trait]
impl ServerApp for TcpProxy {
    async fn process_new(
        self: &Arc<Self>,
        downstream: Stream,
        _shutdown: &ShutdownWatch,
    ) -> Option<Stream> {
        let target = self.target(&downstream)?;
        self.forward(downstream, target, None).await;
        None
    }
}

/// A stream counting the bytes read from it (in its metric, and for its connection)
struct Counted<'a, S> {
    inner: &'a mut S,
    bytes: Option<&'a IntCounter>,
    read: &'a mut u64,
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
            let read = u64::try_from(buf.filled().len() - before).unwrap_or_default();
            if let Some(bytes) = self.bytes {
                bytes.inc_by(read);
            }
            *self.read += read;
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
