//! The requests being served, for `GET /api/v1/requests/stream` (ADR 0008). While no one
//! watches, a request costs one atomic load: nothing is built or sent. While someone watches,
//! at most 100 requests a second (for each watcher) are built, of the hosts they watch: past
//! that, a request costs a few atomic loads.

use std::{
    sync::{
        atomic::{AtomicU64, AtomicUsize, Ordering},
        Arc, OnceLock,
    },
    time::{SystemTime, UNIX_EPOCH},
};

use arc_swap::ArcSwap;
use serde::Serialize;
use tokio::sync::broadcast;

/// Requests a slow watcher can fall behind by before it skips some
const BUFFERED: usize = 1024;

/// Requests built each second, for each watcher
pub const PER_SECOND: u64 = 100;

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

/// Requests built in the current second, and that second
static BUILT: AtomicU64 = AtomicU64::new(0);
static SECOND: AtomicU64 = AtomicU64::new(0);

/// The host each watcher watches (`None`: every host)
fn hosts() -> &'static ArcSwap<Vec<Option<String>>> {
    static HOSTS: OnceLock<ArcSwap<Vec<Option<String>>>> = OnceLock::new();
    HOSTS.get_or_init(|| ArcSwap::from_pointee(vec![]))
}

fn feed() -> &'static broadcast::Sender<Arc<Served>> {
    static FEED: OnceLock<broadcast::Sender<Arc<Served>>> = OnceLock::new();
    FEED.get_or_init(|| broadcast::channel(BUFFERED).0)
}

/// Whether a request to `host` should be built: someone watches it, and this second's budget
/// isn't spent
#[inline]
pub fn wanted(host: &str) -> bool {
    let watchers = WATCHERS.load(Ordering::Relaxed) as u64;
    if watchers == 0 || BUILT.load(Ordering::Relaxed) >= watchers * PER_SECOND {
        return false;
    }
    let watched = hosts().load().iter().any(|watched| {
        watched
            .as_deref()
            .is_none_or(|watched| watched.eq_ignore_ascii_case(host))
    });
    watched && BUILT.fetch_add(1, Ordering::Relaxed) < watchers * PER_SECOND
}

/// Starts a new second's budget, once a second (called by every watcher)
pub fn new_second() {
    let now = Served::now() / 1000;
    if SECOND.swap(now, Ordering::Relaxed) != now {
        BUILT.store(0, Ordering::Relaxed);
    }
}

/// Sends a request to whoever watches
pub fn publish(served: Served) {
    let _ = feed().send(Arc::new(served));
}

/// Requests from now on, until dropped
pub struct Watch {
    pub receiver: broadcast::Receiver<Arc<Served>>,
    host: Option<String>,
}

/// Watches the requests of `host`, or of every host
pub fn watch(host: Option<String>) -> Watch {
    hosts().rcu(|hosts| {
        let mut hosts = hosts.as_ref().clone();
        hosts.push(host.clone());
        hosts
    });
    WATCHERS.fetch_add(1, Ordering::Relaxed);
    Watch {
        receiver: feed().subscribe(),
        host,
    }
}

impl Drop for Watch {
    fn drop(&mut self) {
        WATCHERS.fetch_sub(1, Ordering::Relaxed);
        hosts().rcu(|hosts| {
            let mut hosts = hosts.as_ref().clone();
            if let Some(index) = hosts.iter().position(|host| *host == self.host) {
                hosts.swap_remove(index);
            }
            hosts
        });
    }
}

#[cfg(test)]
mod tests {
    use super::{publish, wanted, watch, Served, PER_SECOND};

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

    // One test: watchers and the budget are global
    #[tokio::test]
    async fn test_watching() {
        assert!(!wanted("a.test"));

        let mut watch = watch(Some("a.test".to_string()));
        // Only the watched host's requests are built
        assert!(wanted("A.test"));
        assert!(!wanted("b.test"));

        publish(served("/a"));
        let received = watch.receiver.recv().await.unwrap();
        assert_eq!(received.path, "/a");

        // At most 100 a second for each watcher
        let built = (0..PER_SECOND * 3).filter(|_| wanted("a.test")).count();
        assert!(built < usize::try_from(PER_SECOND).unwrap(), "{built}");
        assert!(!wanted("a.test"));

        drop(watch);
        assert!(!wanted("a.test"));
    }
}
