use async_trait::async_trait;
use papaya::HashMapRef;
use redis::{Client, Commands};
use std::{error::Error, hash::RandomState, time::Duration};

use crate::{
    certificates::{Certificate, SerializableCertificate},
    notify::{NotifyRecord, NotifySnapshot},
    redis_cursors, redis_history, redis_notify, redis_waiting_room,
    route::{
        HistoryEntry, ReplicaStatus, RouteRecord, RouteVersion, RoutesSnapshot, StoreError, Write,
        DEFAULT_HISTORY,
    },
    store_trait::Store,
    waiting_room::{Admission, RoomLimits},
};

const CHALLENGE_TTL_SECONDS: u64 = 300;

const ROUTE_KEY_PREFIX: &str = "proksi:route:";
const ROUTES_SET_KEY: &str = "proksi:routes";
const REVISION_KEY: &str = "proksi:revision";
const REPLICA_KEY_PREFIX: &str = "proksi:replica:";
const LOCK_KEY_PREFIX: &str = "proksi:lock:";
const CERTIFICATE_KEY_PREFIX: &str = "proksi:cert:";
const CERTIFICATES_REVISION_KEY: &str = "proksi:certificates:revision";
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
    pool: r2d2::Pool<Timed>,
    cache: papaya::HashMap<String, Certificate>,
    challenge_cache: papaya::HashMap<String, (String, String)>,
    /// Versions kept per host (besides the active one)
    history: usize,
}

/// How long connecting to Redis, or a command, may take before it's given up on: a Redis that
/// stops answering must not hold up whoever asks
const TIMEOUT: Duration = Duration::from_secs(2);

/// Connections that time out after `TIMEOUT`: connecting, and every command
pub(crate) struct Timed(Client);

impl r2d2::ManageConnection for Timed {
    type Connection = redis::Connection;
    type Error = redis::RedisError;

    fn connect(&self) -> Result<Self::Connection, Self::Error> {
        let connection = self.0.get_connection_with_timeout(TIMEOUT)?;
        connection.set_read_timeout(Some(TIMEOUT))?;
        connection.set_write_timeout(Some(TIMEOUT))?;
        Ok(connection)
    }

    fn is_valid(&self, connection: &mut Self::Connection) -> Result<(), Self::Error> {
        self.0.is_valid(connection)
    }

    fn has_broken(&self, connection: &mut Self::Connection) -> bool {
        self.0.has_broken(connection)
    }
}

impl RedisStore {
    /// A store on the Redis server at `redis_url`
    ///
    /// # Errors
    ///
    /// When the URL is invalid or the server can't be reached
    pub fn new(redis_url: &str) -> Result<Self, Box<dyn Error>> {
        let client = Client::open(redis_url)?;
        let pool = r2d2::Pool::builder()
            .connection_timeout(TIMEOUT)
            .build(Timed(client))?;

        Ok(RedisStore {
            pool,
            cache: papaya::HashMap::new(),
            challenge_cache: papaya::HashMap::new(),
            history: DEFAULT_HISTORY,
        })
    }

    /// Keeps the last `versions` versions of each host (at least 1)
    #[must_use]
    pub fn with_history(mut self, versions: usize) -> Self {
        self.history = versions.max(1);
        self
    }

    fn connection(&self) -> Result<r2d2::PooledConnection<Timed>, StoreError> {
        self.pool.get().map_err(unavailable)
    }

    fn get_revision(conn: &mut redis::Connection) -> Result<u64, StoreError> {
        let revision: Option<u64> = conn.get(REVISION_KEY).map_err(unavailable)?;
        Ok(revision.unwrap_or(0))
    }

    fn certificate_key(domain: &str) -> String {
        format!("{CERTIFICATE_KEY_PREFIX}{domain}")
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

/// The certificate stored at `key`, unless it can't be read
fn stored_certificate(connection: &mut impl redis::Commands, key: &str) -> Option<Certificate> {
    let data = connection.get::<_, String>(key).ok()?;
    let certificate = serde_json::from_str::<SerializableCertificate>(&data).ok()?;
    Certificate::from_serializable(certificate).ok()
}

/// The record of a route kept as JSON, which includes its schema version
fn record(host: &str, data: String) -> RouteRecord {
    #[derive(serde::Deserialize)]
    struct Version {
        schema_version: u32,
    }
    let schema_version = serde_json::from_str::<Version>(&data).map_or(0, |v| v.schema_version);
    RouteRecord {
        host: host.to_string(),
        schema_version,
        data,
        version: 0,
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

            let versions = redis_history::versions(&mut conn, &hosts)?;

            if Self::get_revision(&mut conn)? != revision {
                continue;
            }

            let routes = hosts
                .iter()
                .zip(values)
                .zip(versions)
                .filter_map(|((host, value), version)| {
                    Some(RouteRecord {
                        version,
                        ..record(host, value?)
                    })
                })
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

    async fn put_route(&self, route: RouteRecord, write: &Write) -> Result<u64, StoreError> {
        redis_history::put(&mut *self.connection()?, &route, write, self.history)
    }

    async fn delete_route(&self, host: &str, write: &Write) -> Result<u64, StoreError> {
        redis_history::delete(&mut *self.connection()?, host, write, self.history)
    }

    async fn rollback_route(&self, host: &str, to: u64, write: &Write) -> Result<u64, StoreError> {
        redis_history::rollback(&mut *self.connection()?, host, to, write, self.history)
    }

    async fn route_history(&self, host: &str) -> Result<Vec<HistoryEntry>, StoreError> {
        redis_history::history(&mut *self.connection()?, host)
    }

    async fn route_version(
        &self,
        host: &str,
        version: u64,
    ) -> Result<Option<RouteVersion>, StoreError> {
        redis_history::version(&mut *self.connection()?, host, version)
    }

    async fn deleted_routes(&self) -> Result<Vec<String>, StoreError> {
        redis_history::deleted(&mut *self.connection()?)
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
            .collect::<Result<_, _>>()
            .map_err(unavailable)?;
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

    async fn try_lock(&self, name: &str, holder: &str, ttl: Duration) -> Result<bool, StoreError> {
        // Taken if free, or extended by its holder; expiry is redis' own (PX)
        let script = redis::Script::new(
            r"
            local current = redis.call('GET', KEYS[1])
            if current == false then
                redis.call('SET', KEYS[1], ARGV[1], 'PX', ARGV[2])
                return 1
            end
            if current == ARGV[1] then
                redis.call('PEXPIRE', KEYS[1], ARGV[2])
                return 1
            end
            return 0
            ",
        );
        let taken: i64 = script
            .key(format!("{LOCK_KEY_PREFIX}{name}"))
            .arg(holder)
            .arg(u64::try_from(ttl.as_millis()).unwrap_or(u64::MAX).max(1))
            .invoke(&mut *self.connection()?)
            .map_err(unavailable)?;
        Ok(taken == 1)
    }

    async fn unlock(&self, name: &str, holder: &str) -> Result<(), StoreError> {
        let script = redis::Script::new(
            r"
            if redis.call('GET', KEYS[1]) == ARGV[1] then
                redis.call('DEL', KEYS[1])
            end
            return 0
            ",
        );
        let _: i64 = script
            .key(format!("{LOCK_KEY_PREFIX}{name}"))
            .arg(holder)
            .invoke(&mut *self.connection()?)
            .map_err(unavailable)?;
        Ok(())
    }

    async fn lock_holder(&self, name: &str) -> Result<Option<String>, StoreError> {
        // Expired keys are gone (PX)
        self.connection()?
            .get(format!("{LOCK_KEY_PREFIX}{name}"))
            .map_err(unavailable)
    }

    async fn wait_in_room(
        &self,
        room: &str,
        visitor: &str,
        limits: RoomLimits,
    ) -> Result<Admission, StoreError> {
        redis_waiting_room::wait(&mut *self.connection()?, room, visitor, limits)
    }

    async fn waiting_room_key(&self) -> Result<[u8; 32], StoreError> {
        redis_waiting_room::key(&mut *self.connection()?)
    }

    async fn advance_cursors(
        &self,
        namespace: &str,
        steps: &[(String, u64)],
    ) -> Result<Vec<u64>, StoreError> {
        redis_cursors::advance(&mut *self.connection()?, namespace, steps)
    }

    async fn notify_revision(&self) -> Result<u64, StoreError> {
        redis_notify::revision(&mut *self.connection()?)
    }

    async fn notify_blocks(&self) -> Result<NotifySnapshot, StoreError> {
        redis_notify::blocks(&mut *self.connection()?)
    }

    async fn put_notify(&self, block: NotifyRecord, write: &Write) -> Result<u64, StoreError> {
        redis_notify::put(&mut *self.connection()?, &block, write)
    }

    async fn delete_notify(&self, name: &str, write: &Write) -> Result<u64, StoreError> {
        redis_notify::delete(&mut *self.connection()?, name, write)
    }

    async fn certificates_revision(&self) -> Result<u64, StoreError> {
        let revision: Option<u64> = self
            .connection()?
            .get(CERTIFICATES_REVISION_KEY)
            .map_err(unavailable)?;
        Ok(revision.unwrap_or(0))
    }

    async fn reload_certificates(&self) -> Result<(), StoreError> {
        let mut conn = self.connection()?;
        let keys: Vec<String> = conn
            .scan_match(format!("{CERTIFICATE_KEY_PREFIX}*"))
            .map_err(unavailable)?
            .collect::<Result<_, _>>()
            .map_err(unavailable)?;
        let values: Vec<Option<String>> = if keys.is_empty() {
            vec![]
        } else {
            conn.mget(&keys).map_err(unavailable)?
        };

        let mut certificates = std::collections::HashMap::new();
        for (key, value) in keys.iter().zip(values) {
            let (Some(domain), Some(value)) = (key.strip_prefix(CERTIFICATE_KEY_PREFIX), value)
            else {
                continue;
            };
            match serde_json::from_str::<SerializableCertificate>(&value)
                .map_err(|err| err.to_string())
                .and_then(|cert| Certificate::from_serializable(cert).map_err(|e| e.to_string()))
            {
                Ok(certificate) => {
                    certificates.insert(domain.to_string(), certificate);
                }
                Err(err) => tracing::warn!("skipping invalid certificate of {domain}: {err}"),
            }
        }

        // Replaced in place, so handshakes always find a certificate
        let cache = self.cache.pin();
        for (domain, certificate) in &certificates {
            cache.insert(domain.clone(), certificate.clone());
        }
        cache.retain(|domain, _| certificates.contains_key(domain));
        Ok(())
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

        redis::pipe()
            .atomic()
            .set(key, cert_json)
            .ignore()
            .incr(CERTIFICATES_REVISION_KEY, 1)
            .ignore()
            .query::<()>(&mut *conn)?;

        // Update cache
        self.cache.pin().insert(domain.to_string(), cert);

        Ok(())
    }

    async fn get_certificates(
        &self,
    ) -> HashMapRef<'_, String, Certificate, RandomState, seize::LocalGuard<'_>> {
        static TEMP_MAP: std::sync::LazyLock<papaya::HashMap<String, Certificate>> =
            std::sync::LazyLock::new(papaya::HashMap::new);

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
            .and_then(Iterator::collect::<Result<Vec<_>, _>>);
        for key in keys.unwrap_or_default() {
            let Some(domain) = key.strip_prefix("proksi:cert:") else {
                continue;
            };
            // Skip if we already have it from cache
            if TEMP_MAP.pin().contains_key(domain) {
                continue;
            }
            let Some(cert) = stored_certificate(&mut *conn_guard, &key) else {
                continue;
            };
            TEMP_MAP.pin().insert(domain.to_string(), cert.clone());
            // Update cache with newly found certificate
            self.cache.pin().insert(domain.to_string(), cert);
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

    #[test]
    fn test_a_server_that_doesnt_answer_times_out() {
        // Accepts connections, and never answers
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        std::thread::spawn(move || {
            let held: Vec<_> = listener.incoming().collect();
            drop(held);
        });

        let (done, finished) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            done.send(RedisStore::new(&format!("redis://127.0.0.1:{port}")).is_err())
                .ok();
        });
        let failed = finished
            .recv_timeout(super::TIMEOUT * 3)
            .expect("still waiting on a server that doesn't answer");
        assert!(failed);
    }

    #[tokio::test]
    async fn test_cursors_contract() {
        if let Some(store) = test_store(6) {
            crate::contract_tests::cursors_contract(&store).await;
        }
    }

    #[tokio::test]
    async fn test_routes_contract() {
        if let Some(store) = test_store(14) {
            crate::contract_tests::routes_contract(&store).await;
        }
    }

    #[tokio::test]
    async fn test_notify_contract() {
        if let Some(store) = test_store(8) {
            crate::contract_tests::notify_contract(&store).await;
        }
    }

    #[tokio::test]
    async fn test_history_contract() {
        if let Some(store) = test_store(9) {
            crate::contract_tests::history_contract(&store.with_history(3)).await;
        }
    }

    #[tokio::test]
    async fn test_replicas_contract() {
        if let Some(store) = test_store(15) {
            crate::contract_tests::replicas_contract(&store).await;
        }
    }

    #[tokio::test]
    async fn test_waiting_rooms_contract() {
        if let Some(store) = test_store(7) {
            crate::contract_tests::waiting_rooms_contract(&store).await;
        }
    }

    #[tokio::test]
    async fn test_leases_contract() {
        if let Some(store) = test_store(11) {
            crate::contract_tests::leases_contract(&store).await;
        }
    }

    #[tokio::test]
    async fn test_certificates_contract() {
        let Some(store) = test_store(10) else {
            return;
        };
        // Another replica
        let url = std::env::var("PROKSI_TEST_REDIS_URL").unwrap();
        let other = RedisStore::new(&format!("{}/10", url.trim_end_matches('/'))).unwrap();
        crate::contract_tests::certificates_contract(&store, &other).await;
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
        let certificate = crate::certificates::Certificate {
            key,
            leaf: leaf.build(),
            chain: vec![],
        };
        crate::store_trait::Store::set_certificate(&store, "cert.test", certificate)
            .await
            .unwrap();

        // Another replica, without the certificate in its cache
        let other = RedisStore::new(&format!(
            "{}/12",
            std::env::var("PROKSI_TEST_REDIS_URL").unwrap()
        ))
        .unwrap();
        let certificates = crate::store_trait::Store::get_certificates(&other).await;

        assert!(certificates.contains_key("cert.test"));
    }

    #[tokio::test]
    async fn test_route_data_is_returned_as_written() {
        let Some(store) = test_store(13) else {
            return;
        };
        let mut conn = store.pool.get().unwrap();
        let _: () = conn.set("proksi:route:broken.test", "{ not json").unwrap();
        let _: () = conn.sadd("proksi:routes", "broken.test").unwrap();

        let snapshot = crate::store_trait::Store::routes(&store).await.unwrap();

        // Proksi reports it as invalid: the store doesn't read it (schema version 0: unknown)
        assert_eq!(
            snapshot.routes,
            vec![crate::RouteRecord {
                host: "broken.test".to_string(),
                schema_version: 0,
                data: "{ not json".to_string(),
                version: 0,
            }]
        );
    }
}
