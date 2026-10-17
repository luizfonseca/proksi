//! Timeouts toward clients (`server.timeouts`), for every HTTP and HTTPS connection, and the
//! defaults toward upstreams

use std::{sync::Arc, time::Duration};

use async_trait::async_trait;
use once_cell::sync::OnceCell;
use pingora::{
    apps::{HttpServerApp, HttpServerOptions, ReusedHttpStream},
    protocols::{http::ServerSession, Stream},
    server::ShutdownWatch,
};

use crate::config::ClientTimeouts;

static TIMEOUTS: OnceCell<ClientTimeouts> = OnceCell::new();

/// Sets the timeouts of the HTTP and HTTPS apps built from now on
pub fn set_client_timeouts(timeouts: ClientTimeouts) {
    let _ = TIMEOUTS.set(timeouts);
}

/// The timeouts toward upstreams of `server.timeouts`
pub fn upstream_timeouts() -> crate::config::host::UpstreamTimeouts {
    TIMEOUTS.get().map(|timeouts| timeouts.upstream).unwrap_or_default()
}

/// An HTTP app whose clients get the configured timeouts: Pingora's own leave connections
/// kept alive, and responses to clients that stop reading, open forever
pub struct WithTimeouts<A> {
    app: Arc<A>,
    timeouts: ClientTimeouts,
    options: HttpServerOptions,
}

impl<A: HttpServerApp> WithTimeouts<A> {
    pub fn new(app: A) -> Self {
        let timeouts = TIMEOUTS.get().copied().unwrap_or_default();
        let mut options = app.server_options().cloned().unwrap_or_default();
        options.h2_idle_timeout = Some(Duration::from_secs(timeouts.idle_secs));
        Self {
            app: Arc::new(app),
            timeouts,
            options,
        }
    }
}

#[async_trait]
impl<A: HttpServerApp + Send + Sync + 'static> HttpServerApp for WithTimeouts<A> {
    async fn process_new_http(
        self: &Arc<Self>,
        mut session: ServerSession,
        shutdown: &ShutdownWatch,
    ) -> Option<ReusedHttpStream> {
        // Waiting for a request (the first, or the next one on a kept-alive connection), unless
        // the connection is closing
        if session.get_keepalive().is_some() {
            session.set_keepalive(Some(self.timeouts.idle_secs));
        }
        session.set_read_timeout(Some(Duration::from_secs(self.timeouts.read_secs)));
        session.set_write_timeout(Some(Duration::from_secs(self.timeouts.write_secs)));
        self.app.process_new_http(session, shutdown).await
    }

    fn h2_options(&self) -> Option<pingora::protocols::http::v2::server::H2Options> {
        self.app.h2_options()
    }

    fn server_options(&self) -> Option<&HttpServerOptions> {
        Some(&self.options)
    }

    async fn http_cleanup(&self) {
        self.app.http_cleanup().await;
    }

    async fn process_custom_session(
        self: Arc<Self>,
        stream: Stream,
        shutdown: &ShutdownWatch,
    ) -> Option<Stream> {
        self.app.clone().process_custom_session(stream, shutdown).await
    }
}
