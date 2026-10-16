//! Addresses with TLS passthrough blocks (ADR 0003): the server name is read from the
//! `ClientHello` before any handshake, then the connection is forwarded untouched to its block,
//! or (on an HTTPS address) handshaken by proksi and served like any HTTPS connection

use std::{sync::Arc, time::Duration};

use async_trait::async_trait;
use pingora::{
    apps::ServerApp,
    protocols::{l4::stream::Stream as L4Stream, tls::server::handshake_with_callback, Stream},
    server::ShutdownWatch,
    tls::ssl::SslAcceptor,
};

use crate::proxy_server::{cert_store::CertStore, services::HttpsApp};

use super::{
    client_hello::{parse, record_length, Hello, RECORD_HEADER},
    passthrough_for,
    proxy::TcpProxy,
};

/// How long a client has to send its `ClientHello`
const HELLO_TIMEOUT: Duration = Duration::from_secs(10);

pub struct SniRouter {
    tcp: TcpProxy,
    /// On an HTTPS address: TLS for the other names, and what serves them
    https: Option<(SslAcceptor, Arc<HttpsApp>)>,
    certificates: CertStore,
}

impl SniRouter {
    pub fn new(tcp: TcpProxy, https: Option<(SslAcceptor, Arc<HttpsApp>)>) -> Self {
        Self {
            tcp,
            https,
            certificates: CertStore::new(),
        }
    }
}

#[async_trait]
impl ServerApp for SniRouter {
    async fn process_new(
        self: &Arc<Self>,
        mut stream: Stream,
        shutdown: &ShutdownWatch,
    ) -> Option<Stream> {
        let local = stream
            .get_socket_digest()
            .and_then(|digest| digest.local_addr().and_then(|addr| addr.as_inet()).copied())?;
        let Hello::ClientHello(server_name) = peek_hello(&mut stream).await? else {
            return None;
        };

        let target = passthrough_for(
            self.tcp.targets(),
            &local,
            server_name.as_deref(),
            self.https.is_some(),
        );
        if let Some(target) = target {
            self.tcp.forward(stream, target, server_name).await;
            return None;
        }

        let (acceptor, https) = self.https.as_ref()?;
        // The listener's own stream type (a plain TCP address)
        let Ok(socket) = stream.into_any().downcast::<L4Stream>() else {
            return None;
        };
        let tls = match handshake_with_callback(acceptor, *socket, &self.certificates).await {
            Ok(tls) => tls,
            Err(err) => {
                tracing::debug!("TLS handshake failed on {local}: {err}");
                return None;
            }
        };
        // Served like a connection of the HTTPS listener, including the following requests
        // of a reused connection
        let mut stream: Stream = Box::new(tls);
        loop {
            stream = https.process_new(stream, shutdown).await?;
        }
    }
}

/// The first record of a connection, without consuming it
async fn peek_hello(stream: &mut Stream) -> Option<Hello> {
    let peek = async {
        let mut header = [0u8; RECORD_HEADER];
        if !stream.try_peek(&mut header).await.ok()? {
            return None;
        }
        let Some(length) = record_length(&header) else {
            return Some(Hello::NotTls);
        };
        let mut record = vec![0u8; RECORD_HEADER + length];
        if !stream.try_peek(&mut record).await.ok()? {
            return None;
        }
        Some(parse(&record))
    };
    tokio::time::timeout(HELLO_TIMEOUT, peek).await.ok()?
}
