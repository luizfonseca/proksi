//! Whether the routes and certificates are in place, so that listening can start: a worker
//! taking over from another (an upgrade) must not accept connections it can't serve yet

use std::{
    collections::HashSet,
    sync::{Mutex, PoisonError},
    time::Duration,
};

use once_cell::sync::Lazy;
use tokio::sync::Notify;

use super::RouteSourceKind;

/// How long listening waits for the routes and certificates, at most
pub const READY_TIMEOUT: Duration = Duration::from_secs(10);

static READINESS: Lazy<Readiness> = Lazy::new(Readiness::default);

/// The sources whose routes have been applied
#[derive(Default)]
pub struct Readiness {
    applied: Mutex<HashSet<RouteSourceKind>>,
    changed: Notify,
}

impl Readiness {
    pub fn applied(&self, source: RouteSourceKind) {
        let mut applied = self.applied.lock().unwrap_or_else(PoisonError::into_inner);
        if applied.insert(source) {
            self.changed.notify_waiters();
        }
    }

    /// Waits until the routes of every source in `sources` have been applied
    pub async fn sources(&self, sources: &[RouteSourceKind]) {
        loop {
            let changed = self.changed.notified();
            {
                let applied = self.applied.lock().unwrap_or_else(PoisonError::into_inner);
                if sources.iter().all(|source| applied.contains(source)) {
                    return;
                }
            }
            changed.await;
        }
    }
}

/// Records that the routes of `source` have been applied (by the reconciler)
pub fn applied(source: RouteSourceKind) {
    READINESS.applied(source);
}

/// Waits until the routes of every source in `sources` have been applied, and with
/// `certificates`, until every host with TLS has its certificate, for up to [`READY_TIMEOUT`].
/// Returns whether everything was ready in time.
pub async fn wait(sources: &[RouteSourceKind], certificates: bool) -> bool {
    let ready = async {
        READINESS.sources(sources).await;
        if certificates {
            while !all_certificates().await {
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
        }
    };
    tokio::time::timeout(READY_TIMEOUT, ready).await.is_ok()
}

/// Whether every host with TLS has its certificate in the store
async fn all_certificates() -> bool {
    let store = crate::stores::global::get_store();
    for host in crate::stores::lookup::tls_hosts() {
        if store.get_certificate(&host.name).await.is_none() {
            return false;
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use crate::routing::RouteSourceKind;

    use super::Readiness;

    #[tokio::test]
    async fn test_waits_for_every_source() {
        let readiness = Readiness::default();
        readiness.applied(RouteSourceKind::File);

        let waiting = readiness.sources(&[RouteSourceKind::File, RouteSourceKind::Docker]);
        tokio::pin!(waiting);
        assert!(
            tokio::time::timeout(Duration::from_millis(100), &mut waiting)
                .await
                .is_err()
        );

        readiness.applied(RouteSourceKind::Docker);
        assert!(tokio::time::timeout(Duration::from_millis(100), waiting)
            .await
            .is_ok());
    }
}
