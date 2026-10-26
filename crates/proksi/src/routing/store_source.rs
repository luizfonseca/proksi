//! Routes kept in the store (shared by every replica)

use crate::config::notify::Kind;
use std::{collections::BTreeMap, sync::Arc, time::Duration};

use proksi_store::{ReplicaStatus, Store, StoreError};

use crate::{notify::Stored, stores::stored_route::HostStore};

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
    /// The revision of the `notify` blocks in use
    notify_revision: Option<u64>,
    /// Where the stored `notify` blocks go: `notify` uses them
    use_notify: fn(BTreeMap<String, Stored>),
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
            notify_revision: None,
            use_notify: crate::notify::configure_stored,
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
        if let Err(err) = self.sync_notify().await {
            tracing::warn!("failed to sync notify blocks from the store: {err}");
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

    /// Uses the `notify` blocks saved in the store, when they changed (blocks that can't be
    /// used are skipped)
    async fn sync_notify(&mut self) -> Result<(), StoreError> {
        let revision = self.store.notify_revision().await?;
        if self.notify_revision == Some(revision) {
            return Ok(());
        }
        let saved = self.store.notify_blocks().await?;
        let blocks = saved
            .blocks
            .iter()
            .filter_map(|record| match crate::notify::stored::decode(record) {
                Ok(stored) => Some((record.name.clone(), stored)),
                Err(err) => {
                    tracing::warn!("skipping notify \"{}\" from the store: {err}", record.name);
                    None
                }
            })
            .collect();
        (self.use_notify)(blocks);
        self.notify_revision = Some(saved.revision);
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
        // Whether the store answered last time: `notify` blocks are told when that changes
        let mut answering = true;
        loop {
            tokio::select! {
                _ = interval.tick() => {}
                () = SYNC_NOW.notified() => {}
            }
            let synced = self.sync_once().await;
            match &synced {
                Ok(Some(revision)) => {
                    tracing::info!("applied routes from the store (revision {revision})");
                }
                Ok(None) => {}
                Err(err) => tracing::warn!(
                    "failed to sync routes from the store, keeping the current routes: {err}"
                ),
            }
            if synced.is_ok() != answering {
                answering = synced.is_ok();
                notify_store_changed(synced.err().map(|err| err.to_string()));
            }
        }
    }
}

/// Tells `notify` blocks the store stopped answering (with why), or answers again (`None`)
fn notify_store_changed(error: Option<String>) {
    let kind = if error.is_some() {
        Kind::StoreDown
    } else {
        Kind::StoreUp
    };
    crate::notify::emit(|| crate::notify::Event {
        detail: error,
        ..crate::notify::Event::new(kind)
    });
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
        collections::BTreeMap,
        sync::{
            atomic::{AtomicBool, AtomicUsize, Ordering},
            Arc, Mutex,
        },
        time::Duration,
    };

    use async_trait::async_trait;
    use papaya::HashMapRef;

    use proksi_store::{
        Certificate, HistoryEntry, MemoryStore, NotifyRecord, NotifySnapshot, ReplicaStatus,
        RouteRecord, RouteVersion, RoutesSnapshot, Store, StoreError, Write,
    };

    use crate::{
        config::notify::{Format, NotifyBlock},
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
        async fn notify_revision(&self) -> Result<u64, StoreError> {
            self.check()?;
            self.inner.notify_revision().await
        }
        async fn notify_blocks(&self) -> Result<NotifySnapshot, StoreError> {
            self.check()?;
            self.inner.notify_blocks().await
        }
        async fn put_notify(&self, block: NotifyRecord, write: &Write) -> Result<u64, StoreError> {
            self.inner.put_notify(block, write).await
        }
        async fn delete_notify(&self, name: &str, write: &Write) -> Result<u64, StoreError> {
            self.inner.delete_notify(name, write).await
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
        async fn advance_cursors(
            &self,
            namespace: &str,
            steps: &[(String, u64)],
        ) -> Result<Vec<u64>, StoreError> {
            self.check()?;
            self.inner.advance_cursors(namespace, steps).await
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
        let mut source = StoreSource::new(store, sender, Duration::from_secs(10));
        // `notify`'s blocks are shared by every test: these leave them alone
        source.use_notify = |_| {};
        (source, receiver)
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
            .set_certificate("a.test", proksi_store::test_util::certificate("a.test", 10))
            .await
            .unwrap();
        source.sync_once().await.unwrap();
        assert_eq!(store.reloads.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn test_store_source_uses_the_notify_blocks_saved() {
        static USED: Mutex<Vec<Vec<String>>> = Mutex::new(Vec::new());
        let store = Arc::new(FlakyStore::default());
        let (mut source, _receiver) = source(store.clone());
        source.use_notify = |blocks| USED.lock().unwrap().push(blocks.into_keys().collect());
        let block = |url: &str| NotifyBlock {
            url: url.to_string(),
            format: Format::Json,
            on: vec![],
            hosts: vec![],
            headers: BTreeMap::new(),
            expiring_days: 14,
        };
        let save =
            |name: &str, url: &str| crate::notify::stored::encode(name, &block(url)).unwrap();
        store
            .put_notify(
                save("ops", "https://hooks.example.com/a"),
                &Write::default(),
            )
            .await
            .unwrap();
        // Saved by an older or newer Proksi, or broken: skipped
        store
            .put_notify(save("broken", "ftp://hooks.example.com"), &Write::default())
            .await
            .unwrap();

        source.sync_once().await.unwrap();
        source.sync_once().await.unwrap();
        // Used once, until they change
        assert_eq!(*USED.lock().unwrap(), vec![vec!["ops".to_string()]]);
        store
            .put_notify(
                save("pager", "https://hooks.example.com/b"),
                &Write::default(),
            )
            .await
            .unwrap();
        source.sync_once().await.unwrap();
        assert_eq!(
            USED.lock().unwrap().last().unwrap(),
            &vec!["ops".to_string(), "pager".to_string()]
        );
    }

    #[tokio::test]
    async fn test_store_source_sends_routes() {
        let store = Arc::new(FlakyStore::default());
        store
            .put_stored(
                &StoredRoute::new(host("a.test", "10.0.0.1")),
                &Write::default(),
            )
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
            .put_stored(
                &StoredRoute::new(host("b.test", "10.0.0.1")),
                &Write::default(),
            )
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
            .put_stored(
                &StoredRoute::new(host("a.test", "10.0.0.1")),
                &Write::default(),
            )
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
            .put_stored(
                &StoredRoute::new(host("a.test", "10.0.0.1")),
                &Write::default(),
            )
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
            .put_stored(
                &StoredRoute::new(host("a.test", "10.0.0.1")),
                &Write::default(),
            )
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
