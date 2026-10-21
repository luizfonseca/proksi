//! Routes kept in the store (shared by every replica)

use std::{sync::Arc, time::Duration};

use proksi_store::{ReplicaStatus, Store, StoreError};

use crate::stores::stored_route::HostStore;

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
    /// Revision of the certificates last loaded from the store
    certificates_revision: Option<u64>,
}

/// Wakes the store sync up, e.g. after the admin API wrote a route, so this instance applies it
/// without waiting for the next interval
static SYNC_NOW: tokio::sync::Notify = tokio::sync::Notify::const_new();

pub fn sync_now() {
    SYNC_NOW.notify_one();
}

/// A replica is considered gone after missing this many syncs
const HEARTBEAT_INTERVALS: u32 = 3;

impl StoreSource {
    pub fn new(store: Arc<dyn Store>, sender: SnapshotSender, interval: Duration) -> Self {
        Self {
            store,
            sender,
            interval,
            replica_id: crate::stores::replica_id().to_string(),
            hostname: crate::stores::hostname(),
            applied_revision: None,
            applied_at: 0,
            certificates_revision: None,
        }
    }

    /// Sends the store's routes if they changed since the last sync, and reports the
    /// applied revision. Returns the revision sent, if any.
    pub async fn sync_once(&mut self) -> Result<Option<u64>, StoreError> {
        let mut sent = None;

        let revision = self.store.revision().await?;
        if self.applied_revision != Some(revision) {
            let stored = self.store.stored_routes().await?;

            let routes = stored
                .routes
                .into_iter()
                .filter_map(|route| {
                    route
                        .map_err(|err| {
                            tracing::warn!(
                                "skipping route for host {} from the store: {}",
                                err.host,
                                err.error
                            );
                        })
                        .ok()
                })
                .map(|stored| stored.route.host)
                .collect::<Vec<_>>();

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

        if let Err(err) = self.sync_certificates().await {
            tracing::warn!("failed to sync certificates from the store: {err}");
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

    /// Reloads the certificates when another replica wrote one (e.g. renewed it)
    async fn sync_certificates(&mut self) -> Result<(), StoreError> {
        let revision = self.store.certificates_revision().await?;
        if self.certificates_revision != Some(revision) {
            self.store.reload_certificates().await?;
            self.certificates_revision = Some(revision);
        }
        Ok(())
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

    /// Syncs every interval (or right away, see `sync_now`), forever
    pub async fn run(mut self) {
        tracing::info!("syncing routes from the store every {:?}", self.interval);
        let mut interval = tokio::time::interval(self.interval);
        loop {
            tokio::select! {
                _ = interval.tick() => {}
                () = SYNC_NOW.notified() => {}
            }
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

#[cfg(test)]
mod tests {
    use std::{
        sync::{
            atomic::{AtomicBool, AtomicUsize, Ordering},
            Arc,
        },
        time::Duration,
    };

    use async_trait::async_trait;
    use papaya::HashMapRef;

    use proksi_store::{
        Certificate, HistoryEntry, MemoryStore, ReplicaStatus, RouteRecord, RouteVersion,
        RoutesSnapshot, Store, StoreError, Write,
    };

    use crate::{
        routing::{test_utils::host, RouteSnapshot, RouteSourceKind},
        stores::stored_route::{HostStore, StoredRoute},
    };

    use super::StoreSource;

    /// Memory store that can be made unavailable
    struct FlakyStore {
        inner: MemoryStore,
        unavailable: AtomicBool,
        /// How many times certificates were reloaded
        reloads: AtomicUsize,
    }

    impl Default for FlakyStore {
        fn default() -> Self {
            Self {
                inner: MemoryStore::new(),
                unavailable: AtomicBool::new(false),
                reloads: AtomicUsize::new(0),
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
        async fn put_route(&self, route: RouteRecord, write: &Write) -> Result<u64, StoreError> {
            self.inner.put_route(route, write).await
        }
        async fn delete_route(&self, host: &str, write: &Write) -> Result<u64, StoreError> {
            self.inner.delete_route(host, write).await
        }
        async fn rollback_route(
            &self,
            host: &str,
            to: u64,
            write: &Write,
        ) -> Result<u64, StoreError> {
            self.inner.rollback_route(host, to, write).await
        }
        async fn route_history(&self, host: &str) -> Result<Vec<HistoryEntry>, StoreError> {
            self.inner.route_history(host).await
        }
        async fn route_version(
            &self,
            host: &str,
            version: u64,
        ) -> Result<Option<RouteVersion>, StoreError> {
            self.inner.route_version(host, version).await
        }
        async fn deleted_routes(&self) -> Result<Vec<String>, StoreError> {
            self.inner.deleted_routes().await
        }
        async fn report(&self, status: ReplicaStatus, ttl: Duration) -> Result<(), StoreError> {
            self.check()?;
            self.inner.report(status, ttl).await
        }
        async fn replicas(&self) -> Result<Vec<ReplicaStatus>, StoreError> {
            self.inner.replicas().await
        }
        async fn try_lock(
            &self,
            name: &str,
            holder: &str,
            ttl: Duration,
        ) -> Result<bool, StoreError> {
            self.check()?;
            self.inner.try_lock(name, holder, ttl).await
        }
        async fn unlock(&self, name: &str, holder: &str) -> Result<(), StoreError> {
            self.check()?;
            self.inner.unlock(name, holder).await
        }
        async fn lock_holder(&self, name: &str) -> Result<Option<String>, StoreError> {
            self.check()?;
            self.inner.lock_holder(name).await
        }
        async fn wait_in_room(
            &self,
            room: &str,
            visitor: &str,
            limits: proksi_store::RoomLimits,
        ) -> Result<proksi_store::Admission, StoreError> {
            self.check()?;
            self.inner.wait_in_room(room, visitor, limits).await
        }
        async fn waiting_room_key(&self) -> Result<[u8; 32], StoreError> {
            self.check()?;
            self.inner.waiting_room_key().await
        }
        async fn certificates_revision(&self) -> Result<u64, StoreError> {
            self.check()?;
            self.inner.certificates_revision().await
        }
        async fn reload_certificates(&self) -> Result<(), StoreError> {
            self.check()?;
            self.reloads.fetch_add(1, Ordering::SeqCst);
            self.inner.reload_certificates().await
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
            .hosts
            .iter()
            .map(|h| h.name.clone())
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

        assert!(
            start.elapsed() < Duration::from_secs(1),
            "{:?}",
            start.elapsed()
        );
    }

    #[tokio::test]
    async fn test_store_source_reloads_certificates_when_they_change() {
        let store = Arc::new(FlakyStore::default());
        let (mut source, _receiver) = source(store.clone());

        // Loaded once at startup
        source.sync_once().await.unwrap();
        assert_eq!(store.reloads.load(Ordering::SeqCst), 1);
        source.sync_once().await.unwrap();
        assert_eq!(store.reloads.load(Ordering::SeqCst), 1);

        // Written by another replica
        store
            .set_certificate(
                "a.test",
                proksi_store::test_util::certificate("a.test", 10),
            )
            .await
            .unwrap();
        source.sync_once().await.unwrap();
        assert_eq!(store.reloads.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn test_store_source_sends_routes() {
        let store = Arc::new(FlakyStore::default());
        store
            .put_stored(&StoredRoute::new(host("a.test", "10.0.0.1")), &Write::default())
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
            .put_stored(&StoredRoute::new(host("b.test", "10.0.0.1")), &Write::default())
            .await
            .unwrap();

        assert_eq!(source.sync_once().await, Ok(Some(1)));
        assert_eq!(hosts(&receiver.try_recv().unwrap()), vec!["b.test"]);
    }

    #[tokio::test]
    async fn test_store_source_skips_unreadable_routes() {
        let store = Arc::new(FlakyStore::default());
        let mut newer = StoredRoute::new(host("newer.test", "10.0.0.1"));
        newer.schema_version = crate::stores::stored_route::ROUTE_SCHEMA_VERSION + 1;
        store.put_stored(&newer, &Write::default()).await.unwrap();
        store
            .put_stored(&StoredRoute::new(host("a.test", "10.0.0.1")), &Write::default())
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
            .put_stored(&StoredRoute::new(host("a.test", "10.0.0.1")), &Write::default())
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
            .put_stored(&StoredRoute::new(host("a.test", "10.0.0.1")), &Write::default())
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
