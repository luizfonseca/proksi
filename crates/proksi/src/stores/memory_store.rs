use async_trait::async_trait;
use papaya::HashMapRef;
use std::{
    collections::{BTreeMap, HashMap},
    error::Error,
    hash::RandomState,
    sync::{
        atomic::{AtomicU64, Ordering},
        Mutex, MutexGuard, PoisonError,
    },
    time::{Duration, Instant},
};

use super::certificates::Certificate;
use super::store_trait::Store;
use super::stored_route::{
    check_schema_version, ReplicaStatus, RoutesSnapshot, StoreError, StoredRoute,
};

pub struct MemoryStore {
    /// Map of domain names to certificates (including leaf & chain)
    inner_certs: papaya::HashMap<String, Certificate>,
    /// Map of domain names to challenge tokens and proofs (token, proof)
    inner_challenges: papaya::HashMap<String, (String, String)>,
    /// Routes, their revision and replicas (changed together)
    route_data: Mutex<MemoryRoutes>,
    /// Leases, by name: their holder and when they expire
    leases: Mutex<HashMap<String, (String, Instant)>>,
    certificates_revision: AtomicU64,
}

#[derive(Default)]
struct MemoryRoutes {
    revision: u64,
    routes: BTreeMap<String, StoredRoute>,
    replicas: HashMap<String, (ReplicaStatus, Instant)>,
}

impl MemoryStore {
    pub fn new() -> Self {
        MemoryStore {
            inner_certs: papaya::HashMap::new(),
            inner_challenges: papaya::HashMap::new(),
            route_data: Mutex::new(MemoryRoutes::default()),
            leases: Mutex::new(HashMap::new()),
            certificates_revision: AtomicU64::new(0),
        }
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
        Ok(RoutesSnapshot {
            revision: routes.revision,
            routes: routes
                .routes
                .values()
                .cloned()
                .map(check_schema_version)
                .collect(),
        })
    }

    async fn revision(&self) -> Result<u64, StoreError> {
        Ok(self.routes().revision)
    }

    async fn put_route(&self, route: StoredRoute) -> Result<u64, StoreError> {
        let mut routes = self.routes();
        routes.routes.insert(route.key(), route);
        routes.revision += 1;
        Ok(routes.revision)
    }

    async fn delete_route(&self, host: &str) -> Result<u64, StoreError> {
        let mut routes = self.routes();
        if routes.routes.remove(&host.to_lowercase()).is_some() {
            routes.revision += 1;
        }
        Ok(routes.revision)
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
    use crate::stores::certificates::Certificate;
    use crate::stores::store_trait::Store;
    use crate::stores::MemoryStore;
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
            chain: None,
        }
    }

    #[tokio::test]
    async fn test_routes_contract() {
        crate::stores::contract_tests::routes_contract(&MemoryStore::new()).await;
    }

    #[tokio::test]
    async fn test_replicas_contract() {
        crate::stores::contract_tests::replicas_contract(&MemoryStore::new()).await;
    }

    #[tokio::test]
    async fn test_leases_contract() {
        crate::stores::contract_tests::leases_contract(&MemoryStore::new()).await;
    }

    #[tokio::test]
    async fn test_certificates_revision() {
        let store = MemoryStore::new();
        let certificate = crate::stores::contract_tests::certificate("a.test", 10);

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
