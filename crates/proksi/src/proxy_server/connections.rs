//! Open connections, so that stopping proksi waits for them to finish, and no longer

use std::{
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
    time::Duration,
};

use async_trait::async_trait;
use pingora::{apps::ServerApp, protocols::Stream, server::ShutdownWatch};

/// The connections of every HTTP, HTTPS and `tcp` listener
pub static CONNECTIONS: Connections = Connections::new();

/// How often open connections are counted while stopping
const DRAIN_CHECK: Duration = Duration::from_millis(100);

/// A count of open connections
pub struct Connections(AtomicUsize);

/// An open connection, counted until dropped
pub struct Open<'a>(&'a Connections);

impl Drop for Open<'_> {
    fn drop(&mut self) {
        (self.0).0.fetch_sub(1, Ordering::Relaxed);
    }
}

impl Connections {
    pub const fn new() -> Self {
        Self(AtomicUsize::new(0))
    }

    pub fn open(&self) -> Open<'_> {
        self.0.fetch_add(1, Ordering::Relaxed);
        Open(self)
    }

    pub fn count(&self) -> usize {
        self.0.load(Ordering::Relaxed)
    }

    /// Waits until every connection is closed, or `timeout`. Returns the connections still open.
    pub async fn drain(&self, timeout: Duration) -> usize {
        let wait = async {
            while self.count() > 0 {
                tokio::time::sleep(DRAIN_CHECK).await;
            }
        };
        let _ = tokio::time::timeout(timeout, wait).await;
        self.count()
    }
}

/// Stops proksi once the server starts shutting down and the connections are closed, or after
/// `timeout`: Pingora would otherwise wait for its whole grace period
pub async fn exit_when_drained(mut shutdown: ShutdownWatch, timeout: Duration) {
    while !*shutdown.borrow() {
        if shutdown.changed().await.is_err() {
            return;
        }
    }
    tracing::info!(
        "stopping: waiting up to {}s for {} open connections to close",
        timeout.as_secs(),
        CONNECTIONS.count()
    );
    crate::notify::emit(|| crate::notify::Event {
        detail: Some(format!("{} open connections", CONNECTIONS.count())),
        ..crate::notify::Event::new(crate::config::notify::Kind::InstanceStopping)
    });
    let open = CONNECTIONS.drain(timeout).await;
    if open > 0 {
        tracing::warn!("stopping: closing {open} connections still open");
    } else {
        tracing::info!("stopping: every connection closed");
    }
    // Lets the last access logs be written
    tokio::time::sleep(Duration::from_millis(200)).await;
    std::process::exit(0);
}

/// An app whose connections are counted in [`CONNECTIONS`]
pub struct Counted<A>(Arc<A>);

impl<A> Counted<A> {
    pub fn new(app: A) -> Self {
        Self(Arc::new(app))
    }
}

#[async_trait]
impl<A: ServerApp + Send + Sync + 'static> ServerApp for Counted<A> {
    async fn process_new(
        self: &Arc<Self>,
        stream: Stream,
        shutdown: &ShutdownWatch,
    ) -> Option<Stream> {
        let _open = CONNECTIONS.open();
        self.0.process_new(stream, shutdown).await
    }

    async fn cleanup(&self) {
        self.0.cleanup().await;
    }
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use super::Connections;

    #[tokio::test]
    async fn test_drain_returns_once_connections_close() {
        let connections = Connections::new();
        assert_eq!(connections.drain(Duration::from_secs(5)).await, 0);

        let open = connections.open();
        let started = Instant::now();
        let closing = async {
            tokio::time::sleep(Duration::from_millis(300)).await;
            drop(open);
        };
        let (open, ()) = tokio::join!(connections.drain(Duration::from_secs(5)), closing);

        assert_eq!(open, 0);
        assert!(started.elapsed() < Duration::from_secs(1));
    }

    #[tokio::test]
    async fn test_drain_stops_waiting_after_the_timeout() {
        let connections = Connections::new();
        let _first = connections.open();
        let _second = connections.open();

        assert_eq!(connections.drain(Duration::from_millis(200)).await, 2);
    }
}
