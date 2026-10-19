//! The requests being served, for `GET /api/v1/requests/stream` (ADR 0008). While no one
//! watches, a request costs one atomic load: nothing is built or sent.

use std::{
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc, OnceLock,
    },
    time::{SystemTime, UNIX_EPOCH},
};

use serde::Serialize;
use tokio::sync::broadcast;

/// Requests a slow watcher can fall behind by before it skips some
const BUFFERED: usize = 1024;

/// A request served, with the fields of access logs the overview shows
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Served {
    /// Unix time, in milliseconds
    pub time: u64,
    pub method: String,
    pub host: String,
    pub path: String,
    pub status: u16,
    pub duration_ms: f64,
    pub client_ip: String,
    /// The location that served it, if any
    pub location: Option<String>,
    pub upstream: Option<String>,
}

impl Served {
    /// Now, in Unix milliseconds
    pub fn now() -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|elapsed| u64::try_from(elapsed.as_millis()).unwrap_or(u64::MAX))
            .unwrap_or_default()
    }
}

static WATCHERS: AtomicUsize = AtomicUsize::new(0);

fn feed() -> &'static broadcast::Sender<Arc<Served>> {
    static FEED: OnceLock<broadcast::Sender<Arc<Served>>> = OnceLock::new();
    FEED.get_or_init(|| broadcast::channel(BUFFERED).0)
}

/// Whether anyone watches: the request path builds nothing otherwise
#[inline]
pub fn watched() -> bool {
    WATCHERS.load(Ordering::Relaxed) > 0
}

/// Sends a request to whoever watches
pub fn publish(served: Served) {
    let _ = feed().send(Arc::new(served));
}

/// Requests from now on, until dropped
pub struct Watch {
    pub receiver: broadcast::Receiver<Arc<Served>>,
}

pub fn watch() -> Watch {
    WATCHERS.fetch_add(1, Ordering::Relaxed);
    Watch {
        receiver: feed().subscribe(),
    }
}

impl Drop for Watch {
    fn drop(&mut self) {
        WATCHERS.fetch_sub(1, Ordering::Relaxed);
    }
}

#[cfg(test)]
mod tests {
    use super::{publish, watch, watched, Served};

    fn served(path: &str) -> Served {
        Served {
            time: Served::now(),
            method: "GET".to_string(),
            host: "a.test".to_string(),
            path: path.to_string(),
            status: 200,
            duration_ms: 1.5,
            client_ip: "127.0.0.1".to_string(),
            location: Some("/".to_string()),
            upstream: None,
        }
    }

    #[tokio::test]
    async fn test_watching() {
        let mut watch = watch();
        assert!(watched());

        publish(served("/a"));

        let received = watch.receiver.recv().await.unwrap();
        assert_eq!(received.path, "/a");
    }
}
