//! Routes kept in the store (shared by every replica)

use std::{sync::Arc, time::Duration};

use crate::stores::{
    store_trait::Store,
    stored_route::{ReplicaStatus, StoreError},
};

use super::{RouteSnapshot, RouteSourceKind, SnapshotSender};

/// Polls the store's revision and sends its routes when it changes.
/// After each sync, reports the applied revision (heartbeat) to the store.
pub struct StoreSource {
    store: Arc<dyn Store>,
    sender: SnapshotSender,
    interval: Duration,
    replica_id: String,
    hostname: String,
    /// Revision of the routes last sent to the reconciler, and when
    applied_revision: Option<u64>,
    applied_at: u64,
}

/// A replica is considered gone after missing this many syncs
const HEARTBEAT_INTERVALS: u32 = 3;

impl StoreSource {
    pub fn new(store: Arc<dyn Store>, sender: SnapshotSender, interval: Duration) -> Self {
        let hostname = hostname();
        let suffix = uuid::Uuid::new_v4().simple().to_string();
        Self {
            store,
            sender,
            interval,
            replica_id: format!("{hostname}-{}", suffix.get(..8).unwrap_or(&suffix)),
            hostname,
            applied_revision: None,
            applied_at: 0,
        }
    }

    /// Sends the store's routes if they changed since the last sync, and reports the
    /// applied revision. Returns the revision sent, if any.
    pub async fn sync_once(&mut self) -> Result<Option<u64>, StoreError> {
        let mut sent = None;

        let revision = self.store.revision().await?;
        if self.applied_revision != Some(revision) {
            let stored = self.store.routes().await?;

            let mut routes = Vec::with_capacity(stored.routes.len());
            for route in stored.routes {
                match route {
                    Ok(route) => routes.push(route.route),
                    Err(err) => tracing::warn!(
                        "skipping route for host {} from the store: {}",
                        err.host,
                        err.error
                    ),
                }
            }

            let mut snapshot = RouteSnapshot::new(RouteSourceKind::Store, routes);
            snapshot.revision = Some(stored.revision);
            self.sender
                .send(snapshot)
                .await
                .map_err(|_| StoreError::Unavailable("routing service stopped".to_string()))?;

            self.applied_revision = Some(stored.revision);
            self.applied_at = unix_timestamp();
            sent = Some(stored.revision);
        }

        if let Some(status) = self.replica_status() {
            if let Err(err) = self
                .store
                .report(status, self.interval * HEARTBEAT_INTERVALS)
                .await
            {
                tracing::warn!("failed to report the applied store revision: {err}");
            }
        }

        Ok(sent)
    }

    /// Status reported to the store (once routes were applied)
    pub fn replica_status(&self) -> Option<ReplicaStatus> {
        let revision = self.applied_revision?;
        Some(ReplicaStatus {
            id: self.replica_id.clone(),
            revision,
            applied_at: self.applied_at,
            version: env!("CARGO_PKG_VERSION").to_string(),
            hostname: self.hostname.clone(),
        })
    }

    /// Syncs every interval, forever
    pub async fn run(mut self) {
        tracing::info!("syncing routes from the store every {:?}", self.interval);
        let mut interval = tokio::time::interval(self.interval);
        loop {
            interval.tick().await;
            match self.sync_once().await {
                Ok(Some(revision)) => {
                    tracing::info!("applied routes from the store (revision {revision})");
                }
                Ok(None) => {}
                Err(err) => tracing::warn!(
                    "failed to sync routes from the store, keeping the current routes: {err}"
                ),
            }
        }
    }
}

/// Runs a future on its own (blocking) thread. Store backends may block (e.g. waiting for
/// a connection when redis is down), which must not stall the other background services.
pub fn spawn_isolated<F>(future: F) -> tokio::task::JoinHandle<()>
where
    F: std::future::Future<Output = ()> + Send + 'static,
{
    let handle = tokio::runtime::Handle::current();
    tokio::task::spawn_blocking(move || handle.block_on(future))
}

fn unix_timestamp() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or_default()
}

fn hostname() -> String {
    nix::unistd::gethostname()
        .ok()
        .and_then(|name| name.into_string().ok())
        .unwrap_or_else(|| "proksi".to_string())
}

#[cfg(test)]
mod tests {
    use std::{
        sync::{
            atomic::{AtomicBool, Ordering},
            Arc,
        },
        time::Duration,
    };

    use async_trait::async_trait;
    use papaya::HashMapRef;

    use crate::{
        routing::{test_utils::route, RouteSnapshot, RouteSourceKind},
        stores::{
            certificates::Certificate,
            store_trait::Store,
            stored_route::{ReplicaStatus, RoutesSnapshot, StoreError, StoredRoute},
            MemoryStore,
        },
    };

    use super::StoreSource;

    /// Memory store that can be made unavailable
    struct FlakyStore {
        inner: MemoryStore,
        unavailable: AtomicBool,
    }

    impl Default for FlakyStore {
        fn default() -> Self {
            Self {
                inner: MemoryStore::new(),
                unavailable: AtomicBool::new(false),
            }
        }
    }

    impl FlakyStore {
        fn check(&self) -> Result<(), StoreError> {
            if self.unavailable.load(Ordering::SeqCst) {
                return Err(StoreError::Unavailable("connection refused".into()));
            }
            Ok(())
        }
    }

    #[async_trait]
    impl Store for FlakyStore {
        async fn routes(&self) -> Result<RoutesSnapshot, StoreError> {
            self.check()?;
            self.inner.routes().await
        }
        async fn revision(&self) -> Result<u64, StoreError> {
            self.check()?;
            self.inner.revision().await
        }
        async fn put_route(&self, route: StoredRoute) -> Result<u64, StoreError> {
            self.inner.put_route(route).await
        }
        async fn delete_route(&self, host: &str) -> Result<u64, StoreError> {
            self.inner.delete_route(host).await
        }
        async fn report(&self, status: ReplicaStatus, ttl: Duration) -> Result<(), StoreError> {
            self.check()?;
            self.inner.report(status, ttl).await
        }
        async fn replicas(&self) -> Result<Vec<ReplicaStatus>, StoreError> {
            self.inner.replicas().await
        }
        async fn get_certificate(&self, domain: &str) -> Option<Certificate> {
            self.inner.get_certificate(domain).await
        }
        async fn set_certificate(
            &self,
            domain: &str,
            cert: Certificate,
        ) -> Result<(), Box<dyn std::error::Error>> {
            self.inner.set_certificate(domain, cert).await
        }
        async fn get_certificates(
            &self,
        ) -> HashMapRef<'_, String, Certificate, std::hash::RandomState, seize::LocalGuard<'_>>
        {
            self.inner.get_certificates().await
        }
        async fn get_challenge(&self, domain: &str) -> Option<(String, String)> {
            self.inner.get_challenge(domain).await
        }
        async fn set_challenge(
            &self,
            domain: &str,
            token: String,
            proof: String,
        ) -> Result<(), Box<dyn std::error::Error>> {
            self.inner.set_challenge(domain, token, proof).await
        }
    }

    fn source(store: Arc<FlakyStore>) -> (StoreSource, tokio::sync::mpsc::Receiver<RouteSnapshot>) {
        let (sender, receiver) = tokio::sync::mpsc::channel(8);
        (
            StoreSource::new(store, sender, Duration::from_secs(10)),
            receiver,
        )
    }

    fn hosts(snapshot: &RouteSnapshot) -> Vec<String> {
        let mut hosts = snapshot
            .routes
            .iter()
            .map(|r| r.host.to_string())
            .collect::<Vec<_>>();
        hosts.sort();
        hosts
    }

    #[tokio::test(flavor = "current_thread")]
    async fn test_spawn_isolated_doesnt_block_other_tasks() {
        let start = std::time::Instant::now();

        super::spawn_isolated(async {
            // A store blocking its thread
            std::thread::sleep(Duration::from_secs(2));
        });
        tokio::time::sleep(Duration::from_millis(10)).await;

        assert!(start.elapsed() < Duration::from_secs(1), "{:?}", start.elapsed());
    }

    #[tokio::test]
    async fn test_store_source_sends_routes() {
        let store = Arc::new(FlakyStore::default());
        store
            .put_route(StoredRoute::new(route("a.test", "10.0.0.1")))
            .await
            .unwrap();
        let (mut source, mut receiver) = source(store.clone());

        assert_eq!(source.sync_once().await, Ok(Some(1)));

        let snapshot = receiver.try_recv().unwrap();
        assert_eq!(snapshot.source, RouteSourceKind::Store);
        assert_eq!(snapshot.revision, Some(1));
        assert_eq!(hosts(&snapshot), vec!["a.test"]);
    }

    #[tokio::test]
    async fn test_store_source_sends_nothing_when_unchanged() {
        let store = Arc::new(FlakyStore::default());
        let (mut source, mut receiver) = source(store.clone());
        source.sync_once().await.unwrap();
        receiver.try_recv().unwrap();

        assert_eq!(source.sync_once().await, Ok(None));
        assert!(receiver.try_recv().is_err());
    }

    #[tokio::test]
    async fn test_store_source_sends_changes() {
        let store = Arc::new(FlakyStore::default());
        let (mut source, mut receiver) = source(store.clone());
        source.sync_once().await.unwrap();
        receiver.try_recv().unwrap();

        store
            .put_route(StoredRoute::new(route("b.test", "10.0.0.1")))
            .await
            .unwrap();

        assert_eq!(source.sync_once().await, Ok(Some(1)));
        assert_eq!(hosts(&receiver.try_recv().unwrap()), vec!["b.test"]);
    }

    #[tokio::test]
    async fn test_store_source_skips_unreadable_routes() {
        let store = Arc::new(FlakyStore::default());
        let mut newer = StoredRoute::new(route("newer.test", "10.0.0.1"));
        newer.schema_version = 2;
        store.put_route(newer).await.unwrap();
        store
            .put_route(StoredRoute::new(route("a.test", "10.0.0.1")))
            .await
            .unwrap();
        let (mut source, mut receiver) = source(store.clone());

        source.sync_once().await.unwrap();

        assert_eq!(hosts(&receiver.try_recv().unwrap()), vec!["a.test"]);
    }

    #[tokio::test]
    async fn test_store_source_keeps_routes_when_store_fails() {
        let store = Arc::new(FlakyStore::default());
        store
            .put_route(StoredRoute::new(route("a.test", "10.0.0.1")))
            .await
            .unwrap();
        let (mut source, mut receiver) = source(store.clone());
        source.sync_once().await.unwrap();
        receiver.try_recv().unwrap();

        store.unavailable.store(true, Ordering::SeqCst);

        assert!(source.sync_once().await.is_err());
        // Nothing sent: the routes from the last sync stay
        assert!(receiver.try_recv().is_err());
    }

    #[tokio::test]
    async fn test_store_source_reports_the_applied_revision() {
        let store = Arc::new(FlakyStore::default());
        store
            .put_route(StoredRoute::new(route("a.test", "10.0.0.1")))
            .await
            .unwrap();
        let (mut source, _receiver) = source(store.clone());

        source.sync_once().await.unwrap();

        let replicas = store.replicas().await.unwrap();
        assert_eq!(replicas.len(), 1);
        assert_eq!(Some(&replicas[0]), source.replica_status().as_ref());
        assert_eq!(replicas[0].revision, 1);
        assert_eq!(replicas[0].version, env!("CARGO_PKG_VERSION"));
        assert!(replicas[0].id.starts_with(&replicas[0].hostname));
    }
}
