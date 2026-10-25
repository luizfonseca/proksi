use async_trait::async_trait;
use papaya::HashMapRef;
use std::{
    collections::{BTreeMap, HashMap, VecDeque},
    error::Error,
    hash::RandomState,
    sync::{
        atomic::{AtomicU64, Ordering},
        Mutex, MutexGuard, PoisonError,
    },
    time::{Duration, Instant},
};

use crate::{
    certificates::Certificate,
    cursors::Cursors,
    notify::{self, NotifyRecord, NotifySnapshot},
    route::{
        Change, Expect, HistoryEntry, ReplicaStatus, RouteRecord, RouteVersion, RoutesSnapshot,
        StoreError, Write, DEFAULT_HISTORY,
    },
    store_trait::Store,
    waiting_room::{self, Admission, Room, RoomLimits},
};

pub struct MemoryStore {
    /// Map of domain names to certificates (including leaf & chain)
    inner_certs: papaya::HashMap<String, Certificate>,
    /// Map of domain names to challenge tokens and proofs (token, proof)
    inner_challenges: papaya::HashMap<String, (String, String)>,
    /// Routes, their revision and replicas (changed together)
    route_data: Mutex<MemoryRoutes>,
    /// `notify` blocks, by name (with the revision each was saved at), and their revision
    notify: Mutex<(u64, BTreeMap<String, NotifyRecord>)>,
    /// Leases, by name: their holder and when they expire
    leases: Mutex<HashMap<String, (String, Instant)>>,
    certificates_revision: AtomicU64,
    /// Versions kept per host (besides the active one)
    history: usize,
    /// Waiting rooms, by name
    rooms: Mutex<HashMap<String, Room>>,
    /// Cursors, by namespace and key
    cursors: Mutex<Cursors>,
    /// The key of waiting rooms' cookies, created on first use
    room_key: Mutex<Option<[u8; 32]>>,
}

#[derive(Default)]
struct MemoryRoutes {
    revision: u64,
    hosts: BTreeMap<String, HostState>,
    replicas: HashMap<String, (ReplicaStatus, Instant)>,
}

/// A host, in the store or deleted, with its versions and changes
#[derive(Default)]
struct HostState {
    /// The version in use, if the host is in the store
    active: Option<u64>,
    versions: BTreeMap<u64, RouteVersion>,
    /// Newest first
    history: VecDeque<HistoryEntry>,
}

impl HostState {
    /// Refuses a change the host doesn't expect
    fn check(&self, expect: Expect) -> Result<(), StoreError> {
        let expected = match expect {
            Expect::Any => true,
            Expect::Absent => self.active.is_none(),
            Expect::Version(version) => self.active.unwrap_or_default() == version,
        };
        if expected {
            return Ok(());
        }
        Err(StoreError::Conflict {
            version: self.active.unwrap_or_default(),
        })
    }

    /// Records a change at `revision`, keeping the last `keep` changes and versions (and the
    /// active version)
    fn record(&mut self, revision: u64, change: Change, write: &Write, keep: usize) {
        self.history.push_front(HistoryEntry {
            revision,
            at: unix_timestamp(),
            author: write.author.clone(),
            change,
        });
        self.history.truncate(keep);
        let removable = self
            .versions
            .keys()
            .copied()
            .filter(|version| Some(*version) != self.active)
            .collect::<Vec<_>>();
        let excess = self.versions.len().saturating_sub(keep);
        for version in removable.into_iter().take(excess) {
            self.versions.remove(&version);
        }
    }
}

fn unix_timestamp() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs())
        .unwrap_or_default()
}

impl Default for MemoryStore {
    fn default() -> Self {
        Self::new()
    }
}

impl MemoryStore {
    /// An empty store, for this replica only
    #[must_use]
    pub fn new() -> Self {
        MemoryStore {
            inner_certs: papaya::HashMap::new(),
            inner_challenges: papaya::HashMap::new(),
            route_data: Mutex::new(MemoryRoutes::default()),
            notify: Mutex::new((0, BTreeMap::new())),
            leases: Mutex::new(HashMap::new()),
            certificates_revision: AtomicU64::new(0),
            history: DEFAULT_HISTORY,
            rooms: Mutex::new(HashMap::new()),
            cursors: Mutex::default(),
            room_key: Mutex::new(None),
        }
    }

    /// Keeps the last `versions` versions of each host (at least 1)
    #[must_use]
    pub fn with_history(mut self, versions: usize) -> Self {
        self.history = versions.max(1);
        self
    }

    fn routes(&self) -> MutexGuard<'_, MemoryRoutes> {
        // The data is still consistent if a thread panicked while holding the lock
        self.route_data
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
    }
}

#[async_trait]
impl Store for MemoryStore {
    async fn routes(&self) -> Result<RoutesSnapshot, StoreError> {
        let routes = self.routes();
        let records = routes
            .hosts
            .values()
            .filter_map(|state| Some(state.versions.get(&state.active?)?.record.clone()));
        Ok(RoutesSnapshot {
            revision: routes.revision,
            routes: records.collect(),
        })
    }

    async fn revision(&self) -> Result<u64, StoreError> {
        Ok(self.routes().revision)
    }

    async fn put_route(&self, route: RouteRecord, write: &Write) -> Result<u64, StoreError> {
        let mut routes = self.routes();
        let host = route.host.to_lowercase();
        routes.hosts.entry(host.clone()).or_default().check(write.expect)?;
        routes.revision += 1;
        let revision = routes.revision;
        let state = routes.hosts.entry(host.clone()).or_default();
        let record = RouteRecord {
            host,
            version: revision,
            ..route
        };
        let version = RouteVersion {
            record,
            at: unix_timestamp(),
            author: write.author.clone(),
        };
        state.versions.insert(revision, version);
        state.active = Some(revision);
        state.record(revision, Change::Saved, write, self.history);
        Ok(revision)
    }

    async fn delete_route(&self, host: &str, write: &Write) -> Result<u64, StoreError> {
        let mut routes = self.routes();
        let revision = routes.revision;
        let Some(state) = routes.hosts.get_mut(&host.to_lowercase()) else {
            return HostState::default().check(write.expect).map(|()| revision);
        };
        state.check(write.expect)?;
        if state.active.take().is_none() {
            return Ok(revision);
        }
        state.record(revision + 1, Change::Deleted, write, self.history);
        routes.revision += 1;
        Ok(routes.revision)
    }

    async fn rollback_route(&self, host: &str, to: u64, write: &Write) -> Result<u64, StoreError> {
        let mut routes = self.routes();
        let revision = routes.revision + 1;
        let state = routes
            .hosts
            .get_mut(&host.to_lowercase())
            .filter(|state| state.versions.contains_key(&to))
            .ok_or(StoreError::NoVersion(to))?;
        state.check(write.expect)?;
        state.active = Some(to);
        state.record(revision, Change::RolledBack { to }, write, self.history);
        routes.revision = revision;
        Ok(revision)
    }

    async fn route_history(&self, host: &str) -> Result<Vec<HistoryEntry>, StoreError> {
        let routes = self.routes();
        let state = routes.hosts.get(&host.to_lowercase());
        Ok(state.map(|state| state.history.iter().cloned().collect()).unwrap_or_default())
    }

    async fn route_version(
        &self,
        host: &str,
        version: u64,
    ) -> Result<Option<RouteVersion>, StoreError> {
        let routes = self.routes();
        let state = routes.hosts.get(&host.to_lowercase());
        Ok(state.and_then(|state| state.versions.get(&version).cloned()))
    }

    async fn deleted_routes(&self) -> Result<Vec<String>, StoreError> {
        let routes = self.routes();
        let deleted = routes
            .hosts
            .iter()
            .filter(|(_, state)| state.active.is_none() && !state.versions.is_empty());
        Ok(deleted.map(|(host, _)| host.clone()).collect())
    }

    async fn notify_revision(&self) -> Result<u64, StoreError> {
        Ok(self.notify.lock().unwrap_or_else(PoisonError::into_inner).0)
    }

    async fn notify_blocks(&self) -> Result<NotifySnapshot, StoreError> {
        let notify = self.notify.lock().unwrap_or_else(PoisonError::into_inner);
        Ok(NotifySnapshot {
            revision: notify.0,
            blocks: notify.1.values().cloned().collect(),
        })
    }

    async fn put_notify(&self, mut block: NotifyRecord, write: &Write) -> Result<u64, StoreError> {
        let mut notify = self.notify.lock().unwrap_or_else(PoisonError::into_inner);
        let (revision, blocks) = &mut *notify;
        notify::check(write.expect, blocks.get(&block.name).map(|saved| saved.version))?;
        *revision += 1;
        block.version = *revision;
        blocks.insert(block.name.clone(), block);
        Ok(*revision)
    }

    async fn delete_notify(&self, name: &str, write: &Write) -> Result<u64, StoreError> {
        let mut notify = self.notify.lock().unwrap_or_else(PoisonError::into_inner);
        let (revision, blocks) = &mut *notify;
        notify::check(write.expect, blocks.get(name).map(|saved| saved.version))?;
        if blocks.remove(name).is_some() {
            *revision += 1;
        }
        Ok(*revision)
    }

    async fn report(&self, status: ReplicaStatus, ttl: Duration) -> Result<(), StoreError> {
        let mut routes = self.routes();
        routes
            .replicas
            .insert(status.id.clone(), (status, Instant::now() + ttl));
        Ok(())
    }

    async fn replicas(&self) -> Result<Vec<ReplicaStatus>, StoreError> {
        let mut routes = self.routes();
        let now = Instant::now();
        routes
            .replicas
            .retain(|_, (_, expires_at)| *expires_at > now);
        Ok(routes
            .replicas
            .values()
            .map(|(status, _)| status.clone())
            .collect())
    }

    async fn try_lock(&self, name: &str, holder: &str, ttl: Duration) -> Result<bool, StoreError> {
        let mut leases = self.leases.lock().unwrap_or_else(PoisonError::into_inner);
        let now = Instant::now();
        let free = leases
            .get(name)
            .is_none_or(|(current, expires_at)| current == holder || *expires_at <= now);
        if free {
            leases.insert(name.to_string(), (holder.to_string(), now + ttl));
        }
        Ok(free)
    }

    async fn unlock(&self, name: &str, holder: &str) -> Result<(), StoreError> {
        let mut leases = self.leases.lock().unwrap_or_else(PoisonError::into_inner);
        if leases
            .get(name)
            .is_some_and(|(current, _)| current == holder)
        {
            leases.remove(name);
        }
        Ok(())
    }

    async fn lock_holder(&self, name: &str) -> Result<Option<String>, StoreError> {
        let leases = self.leases.lock().unwrap_or_else(PoisonError::into_inner);
        Ok(leases
            .get(name)
            .filter(|(_, expires_at)| *expires_at > Instant::now())
            .map(|(holder, _)| holder.clone()))
    }

    async fn certificates_revision(&self) -> Result<u64, StoreError> {
        Ok(self.certificates_revision.load(Ordering::SeqCst))
    }

    async fn wait_in_room(
        &self,
        room: &str,
        visitor: &str,
        limits: RoomLimits,
    ) -> Result<Admission, StoreError> {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, waiting_room::millis);
        let mut rooms = self.rooms.lock().unwrap_or_else(PoisonError::into_inner);
        let admission = rooms
            .entry(room.to_string())
            .or_default()
            .wait(visitor, limits, now);
        // Rooms nobody is in are dropped (a waiting visitor is still in it)
        rooms.retain(|_, room| !room.is_empty());
        Ok(admission)
    }

    async fn waiting_room_key(&self) -> Result<[u8; 32], StoreError> {
        let mut key = self.room_key.lock().unwrap_or_else(PoisonError::into_inner);
        match *key {
            Some(key) => Ok(key),
            None => Ok(*key.insert(waiting_room::new_key()?)),
        }
    }

    async fn advance_cursors(
        &self,
        namespace: &str,
        steps: &[(String, u64)],
    ) -> Result<Vec<u64>, StoreError> {
        let mut cursors = self.cursors.lock().unwrap_or_else(PoisonError::into_inner);
        Ok(cursors.advance(namespace, steps))
    }

    async fn reload_certificates(&self) -> Result<(), StoreError> {
        // Not shared: the certificates are already the store's
        Ok(())
    }

    async fn get_certificate(&self, host: &str) -> Option<Certificate> {
        self.inner_certs.pin().get(host).cloned()
    }

    async fn set_certificate(&self, host: &str, cert: Certificate) -> Result<(), Box<dyn Error>> {
        self.inner_certs.pin().insert(host.to_string(), cert);
        self.certificates_revision.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }

    async fn get_certificates(
        &self,
    ) -> HashMapRef<'_, String, Certificate, RandomState, seize::LocalGuard<'_>> {
        self.inner_certs.pin()
    }

    async fn get_challenge(&self, domain: &str) -> Option<(String, String)> {
        self.inner_challenges.pin().get(domain).cloned()
    }

    async fn set_challenge(
        &self,
        domain: &str,
        token: String,
        proof: String,
    ) -> Result<(), Box<dyn Error>> {
        self.inner_challenges
            .pin()
            .insert(domain.to_string(), (token, proof));
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    // No need to import super since we're using specific imports
    use crate::certificates::Certificate;
    use crate::store_trait::Store;
    use crate::MemoryStore;
    use openssl::hash::MessageDigest;
    use openssl::{
        pkey::PKey,
        rsa::Rsa,
        x509::{X509Name, X509},
    };

    // Helper function to create a test certificate
    fn create_test_certificate(domain: &str) -> Certificate {
        // Create a key pair
        let rsa = Rsa::generate(2048).unwrap();
        let key = PKey::from_rsa(rsa).unwrap();

        // Create a certificate
        let mut name = X509Name::builder().unwrap();
        name.append_entry_by_text("CN", domain).unwrap();
        let name = name.build();

        let mut cert_builder = X509::builder().unwrap();
        cert_builder.set_version(2).unwrap();
        cert_builder.set_subject_name(&name).unwrap();
        cert_builder.set_issuer_name(&name).unwrap();
        cert_builder.set_pubkey(&key).unwrap();

        // Set validity period
        let not_before = openssl::asn1::Asn1Time::days_from_now(0).unwrap();
        let not_after = openssl::asn1::Asn1Time::days_from_now(365).unwrap();
        cert_builder.set_not_before(&not_before).unwrap();
        cert_builder.set_not_after(&not_after).unwrap();

        // Sign the certificate
        cert_builder.sign(&key, MessageDigest::sha256()).unwrap();
        let cert = cert_builder.build();

        Certificate {
            key,
            leaf: cert,
            chain: vec![],
        }
    }

    #[tokio::test]
    async fn test_routes_contract() {
        crate::contract_tests::routes_contract(&MemoryStore::new()).await;
    }

    #[tokio::test]
    async fn test_notify_contract() {
        crate::contract_tests::notify_contract(&MemoryStore::new()).await;
    }

    #[tokio::test]
    async fn test_history_contract() {
        crate::contract_tests::history_contract(&MemoryStore::new().with_history(3)).await;
    }

    #[tokio::test]
    async fn test_replicas_contract() {
        crate::contract_tests::replicas_contract(&MemoryStore::new()).await;
    }

    #[tokio::test]
    async fn test_waiting_rooms_contract() {
        crate::contract_tests::waiting_rooms_contract(&MemoryStore::new()).await;
    }

    #[tokio::test]
    async fn test_leases_contract() {
        crate::contract_tests::leases_contract(&MemoryStore::new()).await;
    }

    #[tokio::test]
    async fn test_cursors_contract() {
        crate::contract_tests::cursors_contract(&MemoryStore::new()).await;
    }

    #[tokio::test]
    async fn test_certificates_revision() {
        let store = MemoryStore::new();
        let certificate = crate::test_util::certificate("a.test", 10);

        assert_eq!(store.certificates_revision().await.unwrap(), 0);
        store.set_certificate("a.test", certificate).await.unwrap();
        assert_eq!(store.certificates_revision().await.unwrap(), 1);
        // Nothing to reload: the memory store isn't shared
        store.reload_certificates().await.unwrap();
        assert!(store.get_certificate("a.test").await.is_some());
    }

    #[tokio::test]
    async fn test_certificate_storage() {
        let store = MemoryStore::new();
        let domain = "example.com";
        let cert = create_test_certificate(domain);

        // Test set_certificate
        store.set_certificate(domain, cert.clone()).await.unwrap();

        // Test get_certificate
        let retrieved_cert = store.get_certificate(domain).await;
        assert!(retrieved_cert.is_some());

        // Test get_certificates
        let all_certs = store.get_certificates().await;
        assert_eq!(all_certs.len(), 1);
        assert!(all_certs.contains_key(domain));
    }

    #[tokio::test]
    async fn test_challenge_storage() {
        let store = MemoryStore::new();
        let domain = "example.com";
        let token = "test-token".to_string();
        let proof = "test-proof".to_string();

        // Test set_challenge
        store
            .set_challenge(domain, token.clone(), proof.clone())
            .await
            .unwrap();

        // Test get_challenge
        let challenge = store.get_challenge(domain).await;
        assert!(challenge.is_some());
        let (retrieved_token, retrieved_proof) = challenge.unwrap();
        assert_eq!(retrieved_token, token);
        assert_eq!(retrieved_proof, proof);
    }

    #[tokio::test]
    async fn test_multiple_domains() {
        let store = MemoryStore::new();

        // Add certificates for multiple domains
        let domains = vec!["example.com", "test.com", "domain.org"];

        for domain in &domains {
            let cert = create_test_certificate(domain);
            store.set_certificate(domain, cert).await.unwrap();

            let token = format!("{}-token", domain);
            let proof = format!("{}-proof", domain);
            store.set_challenge(domain, token, proof).await.unwrap();
        }

        // Verify all certificates are stored
        let all_certs = store.get_certificates().await;
        assert_eq!(all_certs.len(), domains.len());

        // Verify each domain has the correct certificate and challenge
        for domain in &domains {
            let cert = store.get_certificate(domain).await;
            assert!(cert.is_some());

            let challenge = store.get_challenge(domain).await;
            assert!(challenge.is_some());
            let (token, proof) = challenge.unwrap();
            assert_eq!(token, format!("{}-token", domain));
            assert_eq!(proof, format!("{}-proof", domain));
        }
    }

    #[tokio::test]
    async fn test_overwrite_challenge() {
        let store = MemoryStore::new();
        let domain = "example.com";

        // Set initial challenge
        store
            .set_challenge(domain, "token1".to_string(), "proof1".to_string())
            .await
            .unwrap();

        // Overwrite with new challenge
        store
            .set_challenge(domain, "token2".to_string(), "proof2".to_string())
            .await
            .unwrap();

        // Verify the new challenge is stored
        let challenge = store.get_challenge(domain).await.unwrap();
        assert_eq!(challenge.0, "token2");
        assert_eq!(challenge.1, "proof2");
    }

    #[tokio::test]
    async fn test_nonexistent_data() {
        let store = MemoryStore::new();

        // Test getting nonexistent certificate
        let cert = store.get_certificate("nonexistent.com").await;
        assert!(cert.is_none());

        // Test getting nonexistent challenge
        let challenge = store.get_challenge("nonexistent.com").await;
        assert!(challenge.is_none());
    }
}
