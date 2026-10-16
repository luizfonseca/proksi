use async_trait::async_trait;
use papaya::HashMapRef;
use redis::{Client, Commands};
use serde_json;
use std::{error::Error, hash::RandomState, time::Duration};

use super::certificates::{Certificate, SerializableCertificate};
use super::store_trait::Store;
use super::stored_route::{
    decode_stored_route, ReplicaStatus, RoutesSnapshot, StoreError, StoredRoute,
};

const CHALLENGE_TTL_SECONDS: u64 = 300;

const ROUTE_KEY_PREFIX: &str = "proksi:route:";
const ROUTES_SET_KEY: &str = "proksi:routes";
const REVISION_KEY: &str = "proksi:revision";
const REPLICA_KEY_PREFIX: &str = "proksi:replica:";
const ROUTES_READ_ATTEMPTS: usize = 3;

fn route_key(host: &str) -> String {
    format!("{ROUTE_KEY_PREFIX}{host}")
}

fn replica_key(id: &str) -> String {
    format!("{REPLICA_KEY_PREFIX}{id}")
}

#[allow(clippy::needless_pass_by_value)]
fn unavailable(err: impl std::fmt::Display) -> StoreError {
    StoreError::Unavailable(err.to_string())
}

pub struct RedisStore {
    pool: r2d2::Pool<redis::Client>,
    cache: papaya::HashMap<String, Certificate>,
    challenge_cache: papaya::HashMap<String, (String, String)>,
}

impl RedisStore {
    pub fn new(redis_url: &str) -> Result<Self, Box<dyn Error>> {
        let client = Client::open(redis_url)?;
        let pool = r2d2::Pool::builder().build(client)?;

        Ok(RedisStore {
            pool,
            cache: papaya::HashMap::new(),
            challenge_cache: papaya::HashMap::new(),
        })
    }

    fn connection(&self) -> Result<r2d2::PooledConnection<redis::Client>, StoreError> {
        self.pool.get().map_err(unavailable)
    }

    fn get_revision(conn: &mut redis::Connection) -> Result<u64, StoreError> {
        let revision: Option<u64> = conn.get(REVISION_KEY).map_err(unavailable)?;
        Ok(revision.unwrap_or(0))
    }

    fn certificate_key(domain: &str) -> String {
        format!("proksi:cert:{domain}")
    }

    fn challenge_key(domain: &str) -> String {
        format!("proksi:challenge:{domain}")
    }

    fn load_from_redis(&self, domain: &str) -> Option<Certificate> {
        let mut conn = self.pool.get().ok()?;
        let key = Self::certificate_key(domain);

        let cert_data: Option<String> = conn.get(&key).ok()?;

        if let Some(data) = cert_data {
            if let Ok(serializable_cert) = serde_json::from_str::<SerializableCertificate>(&data) {
                return Certificate::from_serializable(serializable_cert).ok();
            }
        }
        None
    }

    fn load_challenge_from_redis(&self, domain: &str) -> Option<(String, String)> {
        let mut conn = self.pool.get().ok()?;
        let key = Self::challenge_key(domain);

        let challenge_data: Option<String> = conn.get(&key).ok()?;

        if let Some(data) = challenge_data {
            if let Ok(challenge) = serde_json::from_str::<(String, String)>(&data) {
                return Some(challenge);
            }
        }
        None
    }
}

#[async_trait]
impl Store for RedisStore {
    async fn routes(&self) -> Result<RoutesSnapshot, StoreError> {
        let mut conn = self.connection()?;

        // Retried if the routes change while being read
        for _ in 0..ROUTES_READ_ATTEMPTS {
            let revision = Self::get_revision(&mut conn)?;
            let hosts: Vec<String> = conn.smembers(ROUTES_SET_KEY).map_err(unavailable)?;
            let keys = hosts.iter().map(|h| route_key(h)).collect::<Vec<_>>();
            let values: Vec<Option<String>> = if keys.is_empty() {
                vec![]
            } else {
                conn.mget(&keys).map_err(unavailable)?
            };

            if Self::get_revision(&mut conn)? != revision {
                continue;
            }

            let routes = hosts
                .iter()
                .zip(values)
                .filter_map(|(host, value)| Some(decode_stored_route(host, &value?)))
                .collect();
            return Ok(RoutesSnapshot { revision, routes });
        }

        Err(StoreError::Unavailable(
            "routes kept changing while being read".to_string(),
        ))
    }

    async fn revision(&self) -> Result<u64, StoreError> {
        Self::get_revision(&mut *self.connection()?)
    }

    async fn put_route(&self, route: StoredRoute) -> Result<u64, StoreError> {
        let host = route.host();
        let json = serde_json::to_string(&route).map_err(|e| StoreError::Invalid(e.to_string()))?;

        let (revision,): (u64,) = redis::pipe()
            .atomic()
            .set(route_key(&host), json)
            .ignore()
            .sadd(ROUTES_SET_KEY, &host)
            .ignore()
            .incr(REVISION_KEY, 1)
            .query(&mut self.connection()?)
            .map_err(unavailable)?;
        Ok(revision)
    }

    async fn delete_route(&self, host: &str) -> Result<u64, StoreError> {
        let host = host.to_lowercase();
        // The revision only changes when a route was deleted
        let script = redis::Script::new(
            r"
            if redis.call('DEL', KEYS[1]) == 1 then
                redis.call('SREM', KEYS[2], ARGV[1])
                return redis.call('INCR', KEYS[3])
            end
            return tonumber(redis.call('GET', KEYS[3]) or '0')
            ",
        );
        script
            .key(route_key(&host))
            .key(ROUTES_SET_KEY)
            .key(REVISION_KEY)
            .arg(&host)
            .invoke(&mut *self.connection()?)
            .map_err(unavailable)
    }

    async fn report(&self, status: ReplicaStatus, ttl: Duration) -> Result<(), StoreError> {
        let json =
            serde_json::to_string(&status).map_err(|e| StoreError::Invalid(e.to_string()))?;
        self.connection()?
            .set_ex(replica_key(&status.id), json, ttl.as_secs().max(1))
            .map_err(unavailable)
    }

    async fn replicas(&self) -> Result<Vec<ReplicaStatus>, StoreError> {
        let mut conn = self.connection()?;
        let keys: Vec<String> = conn
            .scan_match(format!("{REPLICA_KEY_PREFIX}*"))
            .map_err(unavailable)?
            .collect();
        if keys.is_empty() {
            return Ok(vec![]);
        }

        let values: Vec<Option<String>> = conn.mget(&keys).map_err(unavailable)?;
        Ok(values
            .into_iter()
            .flatten()
            .filter_map(|value| serde_json::from_str(&value).ok())
            .collect())
    }

    async fn get_certificate(&self, domain: &str) -> Option<Certificate> {
        // Check cache first
        if let Some(cert) = self.cache.pin().get(domain) {
            return Some(cert.clone());
        }

        // If not in cache, load from Redis
        if let Some(cert) = self.load_from_redis(domain) {
            // Store in cache for future use
            self.cache.pin().insert(domain.to_string(), cert.clone());
            return Some(cert);
        }

        None
    }

    async fn set_certificate(&self, domain: &str, cert: Certificate) -> Result<(), Box<dyn Error>> {
        let mut conn = self.pool.get()?;
        let key = Self::certificate_key(domain);

        // Update Redis
        let serializable_cert = cert.to_serializable()?;
        let cert_json = serde_json::to_string(&serializable_cert)?;

        conn.set::<String, String, String>(key, cert_json)?;

        // Update cache
        self.cache.pin().insert(domain.to_string(), cert);

        Ok(())
    }

    async fn get_certificates(
        &self,
    ) -> HashMapRef<'_, String, Certificate, RandomState, seize::LocalGuard<'_>> {
        static TEMP_MAP: once_cell::sync::Lazy<papaya::HashMap<String, Certificate>> =
            once_cell::sync::Lazy::new(papaya::HashMap::new);

        TEMP_MAP.pin().clear();

        // First, copy all cached certificates
        for (key, value) in &self.cache.pin() {
            TEMP_MAP.pin().insert(key.clone(), value.clone());
        }

        // Then get any additional certificates from Redis
        let Ok(mut conn_guard) = self.pool.get() else {
            tracing::warn!("failed to get Redis connection, returning cached certificates only");
            return TEMP_MAP.pin();
        };
        let pattern = "proksi:cert:*".to_string();
        // SCAN instead of KEYS, which blocks redis while it goes through every key
        let keys = conn_guard
            .scan_match::<_, String>(&pattern)
            .map(Iterator::collect::<Vec<_>>);
        if let Ok(keys) = keys {
            for key in keys {
                if let Some(domain) = key.strip_prefix("proksi:cert:") {
                    let domain = domain.to_string();
                    // Skip if we already have it from cache
                    if TEMP_MAP.pin().contains_key(&domain) {
                        continue;
                    }

                    if let Ok(cert_data) = conn_guard.get::<_, String>(&key) {
                        if let Ok(serializable_cert) =
                            serde_json::from_str::<SerializableCertificate>(&cert_data)
                        {
                            if let Ok(cert) = Certificate::from_serializable(serializable_cert) {
                                TEMP_MAP.pin().insert(domain.clone(), cert.clone());
                                // Update cache with newly found certificate
                                self.cache.pin().insert(domain, cert);
                            }
                        }
                    }
                }
            }
        }

        TEMP_MAP.pin()
    }

    async fn get_challenge(&self, domain: &str) -> Option<(String, String)> {
        // Check cache first
        if let Some(challenge) = self.challenge_cache.pin().get(domain) {
            return Some(challenge.clone());
        }

        // If not in cache, load from Redis
        if let Some(challenge) = self.load_challenge_from_redis(domain) {
            // Store in cache for future use
            self.challenge_cache
                .pin()
                .insert(domain.to_string(), challenge.clone());
            return Some(challenge);
        }

        None
    }

    async fn set_challenge(
        &self,
        domain: &str,
        token: String,
        proof: String,
    ) -> Result<(), Box<dyn Error>> {
        let mut conn = self.pool.get()?;
        let key = Self::challenge_key(domain);

        // Update Redis
        let challenge_tuple = (token.clone(), proof.clone());
        let challenge_json = serde_json::to_string(&challenge_tuple)?;

        conn.set_ex::<String, String, String>(key, challenge_json, CHALLENGE_TTL_SECONDS)?;

        // Optionally, we could set an expiry time for the challenge
        // conn.expire::<String, ()>(key, ttl_seconds as usize)?;

        // Update cache
        self.challenge_cache
            .pin()
            .insert(domain.to_string(), challenge_tuple);

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use redis::Commands;

    use super::RedisStore;

    /// Store on a dedicated database of the redis set in `PROKSI_TEST_REDIS_URL`
    /// (e.g. `redis://127.0.0.1:6379`), emptied first. Returns `None` when it's not set.
    fn test_store(db: u8) -> Option<RedisStore> {
        let Ok(url) = std::env::var("PROKSI_TEST_REDIS_URL") else {
            eprintln!("PROKSI_TEST_REDIS_URL not set, skipping redis store test");
            return None;
        };
        let store = RedisStore::new(&format!("{}/{db}", url.trim_end_matches('/'))).unwrap();
        let _: () = redis::cmd("FLUSHDB")
            .query(&mut store.pool.get().unwrap())
            .unwrap();
        Some(store)
    }

    #[tokio::test]
    async fn test_routes_contract() {
        if let Some(store) = test_store(14) {
            crate::stores::contract_tests::routes_contract(&store).await;
        }
    }

    #[tokio::test]
    async fn test_replicas_contract() {
        if let Some(store) = test_store(15) {
            crate::stores::contract_tests::replicas_contract(&store).await;
        }
    }

    #[tokio::test]
    async fn test_certificates_are_listed_from_redis() {
        let Some(store) = test_store(12) else {
            return;
        };
        let key =
            openssl::pkey::PKey::from_rsa(openssl::rsa::Rsa::generate(2048).unwrap()).unwrap();
        let mut leaf = openssl::x509::X509::builder().unwrap();
        leaf.set_pubkey(&key).unwrap();
        leaf.set_not_before(&openssl::asn1::Asn1Time::days_from_now(0).unwrap())
            .unwrap();
        leaf.set_not_after(&openssl::asn1::Asn1Time::days_from_now(1).unwrap())
            .unwrap();
        leaf.sign(&key, openssl::hash::MessageDigest::sha256())
            .unwrap();
        let certificate = crate::stores::certificates::Certificate {
            key,
            leaf: leaf.build(),
            chain: None,
        };
        crate::stores::store_trait::Store::set_certificate(&store, "cert.test", certificate)
            .await
            .unwrap();

        // Another replica, without the certificate in its cache
        let other = RedisStore::new(&format!(
            "{}/12",
            std::env::var("PROKSI_TEST_REDIS_URL").unwrap()
        ))
        .unwrap();
        let certificates = crate::stores::store_trait::Store::get_certificates(&other).await;

        assert!(certificates.contains_key("cert.test"));
    }

    #[tokio::test]
    async fn test_invalid_route_data_is_reported() {
        let Some(store) = test_store(13) else {
            return;
        };
        let mut conn = store.pool.get().unwrap();
        let _: () = conn.set("proksi:route:broken.test", "{ not json").unwrap();
        let _: () = conn.sadd("proksi:routes", "broken.test").unwrap();

        let snapshot = crate::stores::store_trait::Store::routes(&store)
            .await
            .unwrap();

        assert_eq!(snapshot.routes.len(), 1);
        assert!(snapshot.routes[0].is_err());
    }
}
