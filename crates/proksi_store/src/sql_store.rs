//! SQL store (sqlite and postgres), through sqlx's `Any` driver

use std::{error::Error, future::Future, hash::RandomState, time::Duration};

use async_trait::async_trait;
use papaya::HashMapRef;
use sqlx::{any::AnyPoolOptions, AnyPool, Row};

use crate::{
    certificates::{Certificate, SerializableCertificate},
    route::{
        HistoryEntry, ReplicaStatus, RouteRecord, RouteVersion, RoutesSnapshot, StoreError, Write,
        DEFAULT_HISTORY,
    },
    sql_history,
    store_trait::Store,
};

const CHALLENGE_TTL_SECONDS: i64 = 300;
const ROUTES_READ_ATTEMPTS: usize = 3;

pub struct SqlStore {
    pool: AnyPool,
    /// The current time in milliseconds, by the database's clock (leases never use replicas'
    /// clocks, which can drift)
    now_ms: &'static str,
    /// Queries run on this runtime: proksi uses several runtimes, and connections
    /// belong to the runtime they were created on
    runtime: Option<tokio::runtime::Runtime>,
    /// Certificates already loaded (also returned by `get_certificates`)
    cache: papaya::HashMap<String, Certificate>,
    /// Versions kept per host (besides the active one)
    history: usize,
}

impl SqlStore {
    /// Connects to the database and creates/updates the tables.
    /// For sqlite, use `sqlite://<path>?mode=rwc` to create the file if needed.
    ///
    /// # Errors
    ///
    /// When the database can't be reached or its tables can't be created
    pub fn connect(database_url: &str) -> Result<Self, StoreError> {
        sqlx::any::install_default_drivers();

        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .thread_name("proksi-store")
            .enable_all()
            .build()
            .map_err(unavailable)?;

        let url = database_url.to_string();
        let pool = block_on(&runtime, async move {
            let pool = AnyPoolOptions::new()
                .max_connections(5)
                .acquire_timeout(Duration::from_secs(5))
                .connect(&url)
                .await
                .map_err(unavailable)?;
            sqlx::migrate!("./migrations")
                .run(&pool)
                .await
                .map_err(unavailable)?;
            Ok::<_, StoreError>(pool)
        })?;

        let now_ms = if database_url.starts_with("sqlite") {
            "CAST((julianday('now') - 2440587.5) * 86400000 AS BIGINT)"
        } else {
            "CAST(EXTRACT(EPOCH FROM clock_timestamp()) * 1000 AS BIGINT)"
        };

        Ok(Self {
            pool,
            now_ms,
            runtime: Some(runtime),
            cache: papaya::HashMap::new(),
            history: DEFAULT_HISTORY,
        })
    }

    /// Keeps the last `versions` versions of each host (at least 1)
    #[must_use]
    pub fn with_history(mut self, versions: usize) -> Self {
        self.history = versions.max(1);
        self
    }

    /// Runs a query on the store's runtime
    fn run<T, F>(&self, future: F) -> impl Future<Output = Result<T, StoreError>>
    where
        T: Send + 'static,
        F: Future<Output = Result<T, StoreError>> + Send + 'static,
    {
        let task = self.runtime.as_ref().map(|runtime| runtime.spawn(future));
        async move {
            match task {
                Some(task) => task.await.map_err(unavailable)?,
                None => Err(StoreError::Unavailable("store closed".to_string())),
            }
        }
    }
}

impl SqlStore {
    /// Reads Let's Encrypt data (blocking, as required by `acme_v2`)
    ///
    /// # Errors
    ///
    /// When the database can't be reached
    pub fn acme_get(&self, key: &str) -> Result<Option<String>, StoreError> {
        let Some(runtime) = self.runtime.as_ref() else {
            return Err(StoreError::Unavailable("store closed".to_string()));
        };
        let pool = self.pool.clone();
        let key = key.to_string();
        block_on(runtime, async move {
            let row = sqlx::query("SELECT value FROM proksi_acme WHERE key = $1")
                .bind(key)
                .fetch_optional(&pool)
                .await
                .map_err(unavailable)?;
            row.map(|row| row.try_get::<String, _>("value").map_err(invalid))
                .transpose()
        })
    }

    /// Writes Let's Encrypt data (blocking, as required by `acme_v2`)
    ///
    /// # Errors
    ///
    /// When the database can't be reached
    pub fn acme_put(&self, key: &str, value: String) -> Result<(), StoreError> {
        let Some(runtime) = self.runtime.as_ref() else {
            return Err(StoreError::Unavailable("store closed".to_string()));
        };
        let pool = self.pool.clone();
        let key = key.to_string();
        block_on(runtime, async move {
            sqlx::query(
                "INSERT INTO proksi_acme (key, value) VALUES ($1, $2) \
                 ON CONFLICT (key) DO UPDATE SET value = excluded.value",
            )
            .bind(key)
            .bind(value)
            .execute(&pool)
            .await
            .map_err(unavailable)?;
            Ok(())
        })
    }
}

impl Drop for SqlStore {
    fn drop(&mut self) {
        if let Some(runtime) = self.runtime.take() {
            runtime.shutdown_background();
        }
    }
}

/// Runs a future to completion on `runtime`, from any context (including another runtime)
fn block_on<T: Send, F: Future<Output = T> + Send>(
    runtime: &tokio::runtime::Runtime,
    future: F,
) -> T {
    std::thread::scope(|scope| {
        scope
            .spawn(|| runtime.block_on(future))
            .join()
            .unwrap_or_else(|panic| std::panic::resume_unwind(panic))
    })
}

#[allow(clippy::needless_pass_by_value)]
fn unavailable(err: impl std::fmt::Display) -> StoreError {
    StoreError::Unavailable(err.to_string())
}

fn invalid(err: impl std::fmt::Display) -> StoreError {
    StoreError::Invalid(err.to_string())
}

fn unix_timestamp() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()
        .and_then(|d| i64::try_from(d.as_secs()).ok())
        .unwrap_or_default()
}

fn to_revision(value: i64) -> Result<u64, StoreError> {
    u64::try_from(value).map_err(invalid)
}

async fn revision<'e, E>(executor: E) -> Result<u64, StoreError>
where
    E: sqlx::Executor<'e, Database = sqlx::Any>,
{
    let row = sqlx::query("SELECT value FROM proksi_meta WHERE key = 'revision'")
        .fetch_one(executor)
        .await
        .map_err(unavailable)?;
    to_revision(row.try_get("value").map_err(invalid)?)
}

/// A stored certificate, unless it can't be read
fn row_certificate(row: &sqlx::any::AnyRow) -> Option<(String, Certificate)> {
    let domain = row.try_get::<String, _>("domain").ok()?;
    let data = row.try_get::<String, _>("data").ok()?;
    let certificate = serde_json::from_str::<SerializableCertificate>(&data).ok()?;
    Some((domain, Certificate::from_serializable(certificate).ok()?))
}

#[async_trait]
impl Store for SqlStore {
    async fn routes(&self) -> Result<RoutesSnapshot, StoreError> {
        let pool = self.pool.clone();
        self.run(async move {
            // Retried if the routes change while being read
            for _ in 0..ROUTES_READ_ATTEMPTS {
                let current = revision(&pool).await?;
                let rows = sqlx::query(
                    "SELECT r.host, r.schema_version, r.data, COALESCE(s.version, 0) AS version \
                     FROM proksi_routes r LEFT JOIN proksi_route_state s ON s.host = r.host",
                )
                    .fetch_all(&pool)
                    .await
                    .map_err(unavailable)?;
                if revision(&pool).await? != current {
                    continue;
                }

                let mut routes = Vec::with_capacity(rows.len());
                for row in rows {
                    let host: String = row.try_get("host").map_err(invalid)?;
                    let schema_version: i64 = row.try_get("schema_version").map_err(invalid)?;
                    let data: String = row.try_get("data").map_err(invalid)?;
                    let version: i64 = row.try_get("version").map_err(invalid)?;
                    routes.push(RouteRecord {
                        host,
                        schema_version: u32::try_from(schema_version).unwrap_or_default(),
                        data,
                        version: u64::try_from(version).unwrap_or_default(),
                    });
                }
                return Ok(RoutesSnapshot {
                    revision: current,
                    routes,
                });
            }
            Err(StoreError::Unavailable(
                "routes kept changing while being read".to_string(),
            ))
        })
        .await
    }

    async fn revision(&self) -> Result<u64, StoreError> {
        let pool = self.pool.clone();
        self.run(async move { revision(&pool).await }).await
    }

    async fn put_route(&self, route: RouteRecord, write: &Write) -> Result<u64, StoreError> {
        let task = sql_history::put(self.pool.clone(), route, write.clone(), self.history);
        self.run(task).await
    }

    async fn delete_route(&self, host: &str, write: &Write) -> Result<u64, StoreError> {
        let task = sql_history::delete(self.pool.clone(), host.to_string(), write.clone(), self.history);
        self.run(task).await
    }

    async fn rollback_route(&self, host: &str, to: u64, write: &Write) -> Result<u64, StoreError> {
        let host = host.to_string();
        let task = sql_history::rollback(self.pool.clone(), host, to, write.clone(), self.history);
        self.run(task).await
    }

    async fn route_history(&self, host: &str) -> Result<Vec<HistoryEntry>, StoreError> {
        self.run(sql_history::history(self.pool.clone(), host.to_string())).await
    }

    async fn route_version(
        &self,
        host: &str,
        version: u64,
    ) -> Result<Option<RouteVersion>, StoreError> {
        self.run(sql_history::version(self.pool.clone(), host.to_string(), version)).await
    }

    async fn deleted_routes(&self) -> Result<Vec<String>, StoreError> {
        self.run(sql_history::deleted(self.pool.clone())).await
    }

    async fn report(&self, status: ReplicaStatus, ttl: Duration) -> Result<(), StoreError> {
        let pool = self.pool.clone();
        let data = serde_json::to_string(&status).map_err(invalid)?;
        let ttl = i64::try_from(ttl.as_secs()).map_err(invalid)?.max(1);
        self.run(async move {
            sqlx::query(
                "INSERT INTO proksi_replicas (id, data, expires_at) VALUES ($1, $2, $3) \
                 ON CONFLICT (id) DO UPDATE SET data = excluded.data, expires_at = excluded.expires_at",
            )
            .bind(status.id)
            .bind(data)
            .bind(unix_timestamp() + ttl)
            .execute(&pool)
            .await
            .map_err(unavailable)?;
            Ok(())
        })
        .await
    }

    async fn replicas(&self) -> Result<Vec<ReplicaStatus>, StoreError> {
        let pool = self.pool.clone();
        self.run(async move {
            let now = unix_timestamp();
            sqlx::query("DELETE FROM proksi_replicas WHERE expires_at <= $1")
                .bind(now)
                .execute(&pool)
                .await
                .map_err(unavailable)?;
            let rows = sqlx::query("SELECT data FROM proksi_replicas WHERE expires_at > $1")
                .bind(now)
                .fetch_all(&pool)
                .await
                .map_err(unavailable)?;
            Ok(rows
                .iter()
                .filter_map(|row| row.try_get::<String, _>("data").ok())
                .filter_map(|data| serde_json::from_str(&data).ok())
                .collect())
        })
        .await
    }

    async fn try_lock(&self, name: &str, holder: &str, ttl: Duration) -> Result<bool, StoreError> {
        let pool = self.pool.clone();
        let (name, holder) = (name.to_string(), holder.to_string());
        let ttl = i64::try_from(ttl.as_millis()).map_err(invalid)?.max(1);
        // Taken if free (or expired), or extended by its holder
        let query = format!(
            "INSERT INTO proksi_locks (name, holder, expires_at) VALUES ($1, $2, {now} + $3) \
             ON CONFLICT (name) DO UPDATE SET holder = excluded.holder, expires_at = excluded.expires_at \
             WHERE proksi_locks.holder = excluded.holder OR proksi_locks.expires_at <= {now}",
            now = self.now_ms
        );
        self.run(async move {
            let taken = sqlx::query(sqlx::AssertSqlSafe(query))
                .bind(name)
                .bind(holder)
                .bind(ttl)
                .execute(&pool)
                .await
                .map_err(unavailable)?
                .rows_affected();
            Ok(taken == 1)
        })
        .await
    }

    async fn unlock(&self, name: &str, holder: &str) -> Result<(), StoreError> {
        let pool = self.pool.clone();
        let (name, holder) = (name.to_string(), holder.to_string());
        self.run(async move {
            sqlx::query("DELETE FROM proksi_locks WHERE name = $1 AND holder = $2")
                .bind(name)
                .bind(holder)
                .execute(&pool)
                .await
                .map_err(unavailable)?;
            Ok(())
        })
        .await
    }

    async fn lock_holder(&self, name: &str) -> Result<Option<String>, StoreError> {
        let pool = self.pool.clone();
        let name = name.to_string();
        let query = format!(
            "SELECT holder FROM proksi_locks WHERE name = $1 AND expires_at > {now}",
            now = self.now_ms
        );
        self.run(async move {
            let row = sqlx::query(sqlx::AssertSqlSafe(query))
                .bind(name)
                .fetch_optional(&pool)
                .await
                .map_err(unavailable)?;
            row.map(|row| row.try_get::<String, _>("holder").map_err(invalid))
                .transpose()
        })
        .await
    }

    async fn certificates_revision(&self) -> Result<u64, StoreError> {
        let pool = self.pool.clone();
        self.run(async move {
            let row =
                sqlx::query("SELECT value FROM proksi_meta WHERE key = 'certificates_revision'")
                    .fetch_one(&pool)
                    .await
                    .map_err(unavailable)?;
            to_revision(row.try_get("value").map_err(invalid)?)
        })
        .await
    }

    async fn reload_certificates(&self) -> Result<(), StoreError> {
        let pool = self.pool.clone();
        let rows = self
            .run(async move {
                sqlx::query("SELECT domain, data FROM proksi_certificates")
                    .fetch_all(&pool)
                    .await
                    .map_err(unavailable)
            })
            .await?;

        let mut certificates = std::collections::HashMap::new();
        for row in rows {
            let domain: String = row.try_get("domain").map_err(invalid)?;
            let data: String = row.try_get("data").map_err(invalid)?;
            match serde_json::from_str::<SerializableCertificate>(&data)
                .map_err(|err| err.to_string())
                .and_then(|cert| Certificate::from_serializable(cert).map_err(|e| e.to_string()))
            {
                Ok(certificate) => {
                    certificates.insert(domain, certificate);
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
        if let Some(certificate) = self.cache.pin().get(domain) {
            return Some(certificate.clone());
        }

        let pool = self.pool.clone();
        let key = domain.to_string();
        let data = self
            .run(async move {
                let row = sqlx::query("SELECT data FROM proksi_certificates WHERE domain = $1")
                    .bind(key)
                    .fetch_optional(&pool)
                    .await
                    .map_err(unavailable)?;
                row.map(|row| row.try_get::<String, _>("data").map_err(invalid))
                    .transpose()
            })
            .await
            .ok()??;

        let certificate = serde_json::from_str::<SerializableCertificate>(&data)
            .ok()
            .and_then(|cert| Certificate::from_serializable(cert).ok())?;
        self.cache
            .pin()
            .insert(domain.to_string(), certificate.clone());
        Some(certificate)
    }

    async fn set_certificate(&self, domain: &str, cert: Certificate) -> Result<(), Box<dyn Error>> {
        let data = serde_json::to_string(&cert.to_serializable()?)?;
        let pool = self.pool.clone();
        let key = domain.to_string();
        self.run(async move {
            let mut tx = pool.begin().await.map_err(unavailable)?;
            sqlx::query(
                "INSERT INTO proksi_certificates (domain, data) VALUES ($1, $2) \
                 ON CONFLICT (domain) DO UPDATE SET data = excluded.data",
            )
            .bind(key)
            .bind(data)
            .execute(&mut *tx)
            .await
            .map_err(unavailable)?;
            sqlx::query(
                "UPDATE proksi_meta SET value = value + 1 WHERE key = 'certificates_revision'",
            )
            .execute(&mut *tx)
            .await
            .map_err(unavailable)?;
            tx.commit().await.map_err(unavailable)?;
            Ok(())
        })
        .await?;

        self.cache.pin().insert(domain.to_string(), cert);
        Ok(())
    }

    async fn get_certificates(
        &self,
    ) -> HashMapRef<'_, String, Certificate, RandomState, seize::LocalGuard<'_>> {
        let pool = self.pool.clone();
        let rows = self
            .run(async move {
                sqlx::query("SELECT domain, data FROM proksi_certificates")
                    .fetch_all(&pool)
                    .await
                    .map_err(unavailable)
            })
            .await;

        match rows {
            Ok(rows) => {
                let cache = self.cache.pin();
                for (domain, certificate) in rows.iter().filter_map(row_certificate) {
                    cache.insert(domain, certificate);
                }
            }
            Err(err) => {
                tracing::warn!("failed to load certificates, returning cached certificates: {err}");
            }
        }

        self.cache.pin()
    }

    async fn get_challenge(&self, domain: &str) -> Option<(String, String)> {
        let pool = self.pool.clone();
        let key = domain.to_string();
        self.run(async move {
            let row = sqlx::query(
                "SELECT token, proof FROM proksi_challenges WHERE domain = $1 AND expires_at > $2",
            )
            .bind(key)
            .bind(unix_timestamp())
            .fetch_optional(&pool)
            .await
            .map_err(unavailable)?;
            row.map(|row| {
                Ok((
                    row.try_get("token").map_err(invalid)?,
                    row.try_get("proof").map_err(invalid)?,
                ))
            })
            .transpose()
        })
        .await
        .ok()
        .flatten()
    }

    async fn set_challenge(
        &self,
        domain: &str,
        token: String,
        proof: String,
    ) -> Result<(), Box<dyn Error>> {
        let pool = self.pool.clone();
        let key = domain.to_string();
        self.run(async move {
            sqlx::query(
                "INSERT INTO proksi_challenges (domain, token, proof, expires_at) \
                 VALUES ($1, $2, $3, $4) \
                 ON CONFLICT (domain) DO UPDATE SET token = excluded.token, \
                 proof = excluded.proof, expires_at = excluded.expires_at",
            )
            .bind(key)
            .bind(token)
            .bind(proof)
            .bind(unix_timestamp() + CHALLENGE_TTL_SECONDS)
            .execute(&pool)
            .await
            .map_err(unavailable)?;
            Ok(())
        })
        .await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use crate::{certificates::Certificate, contract_tests, store_trait::Store};

    use super::SqlStore;

    #[cfg(feature = "sqlite")]
    fn sqlite_store() -> (SqlStore, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let url = format!(
            "sqlite://{}?mode=rwc",
            dir.path().join("proksi.db").to_string_lossy()
        );
        (SqlStore::connect(&url).unwrap(), dir)
    }

    /// Postgres set in `PROKSI_TEST_POSTGRES_URL`, with a dedicated, emptied schema
    #[cfg(feature = "postgres")]
    fn postgres_store(schema: &str) -> Option<SqlStore> {
        let Ok(url) = std::env::var("PROKSI_TEST_POSTGRES_URL") else {
            eprintln!("PROKSI_TEST_POSTGRES_URL not set, skipping postgres store test");
            return None;
        };
        // A schema per test, so tests running in parallel don't interfere
        let admin = SqlStore::connect(&url).unwrap();
        let schema_name = schema.to_string();
        let pool = admin.pool.clone();
        super::block_on(admin.runtime.as_ref().unwrap(), async move {
            sqlx::query(sqlx::AssertSqlSafe(format!(
                "DROP SCHEMA IF EXISTS {schema_name} CASCADE"
            )))
            .execute(&pool)
            .await
            .unwrap();
            sqlx::query(sqlx::AssertSqlSafe(format!("CREATE SCHEMA {schema_name}")))
                .execute(&pool)
                .await
                .unwrap();
        });
        Some(SqlStore::connect(&postgres_url(&url, schema)).unwrap())
    }

    #[cfg(feature = "postgres")]
    fn postgres_url(url: &str, schema: &str) -> String {
        let separator = if url.contains('?') { '&' } else { '?' };
        format!("{url}{separator}options=-c search_path={schema}")
    }

    fn certificate() -> Certificate {
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
        Certificate {
            key,
            leaf: leaf.build(),
            chain: vec![],
        }
    }

    async fn certificates_and_challenges(store: &SqlStore, reopen: impl Fn() -> SqlStore) {
        let cert = certificate();
        store
            .set_certificate("cert.test", cert.clone())
            .await
            .unwrap();
        store
            .set_challenge("cert.test", "token".into(), "proof".into())
            .await
            .unwrap();

        // Read by another replica
        let other = reopen();
        let stored = other.get_certificate("cert.test").await.unwrap();
        assert_eq!(stored.leaf.to_pem().unwrap(), cert.leaf.to_pem().unwrap());
        assert!(other.get_certificates().await.contains_key("cert.test"));
        assert_eq!(
            other.get_challenge("cert.test").await,
            Some(("token".to_string(), "proof".to_string()))
        );
        assert_eq!(
            other.get_certificate("missing.test").await.map(|_| ()),
            None
        );
    }

    #[cfg(feature = "sqlite")]
    #[tokio::test]
    async fn test_sqlite_routes_contract() {
        let (store, _dir) = sqlite_store();
        contract_tests::routes_contract(&store).await;
    }

    #[cfg(feature = "sqlite")]
    #[tokio::test]
    async fn test_sqlite_history_contract() {
        let (store, _dir) = sqlite_store();
        contract_tests::history_contract(&store.with_history(3)).await;
    }

    #[cfg(feature = "postgres")]
    #[tokio::test]
    async fn test_postgres_history_contract() {
        if let Some(store) = postgres_store("proksi_test_history") {
            contract_tests::history_contract(&store.with_history(3)).await;
        }
    }

    #[cfg(feature = "sqlite")]
    #[tokio::test]
    async fn test_sqlite_replicas_contract() {
        let (store, _dir) = sqlite_store();
        contract_tests::replicas_contract(&store).await;
    }

    #[cfg(feature = "sqlite")]
    #[tokio::test]
    async fn test_sqlite_certificates_and_challenges() {
        let (store, dir) = sqlite_store();
        let url = format!(
            "sqlite://{}?mode=rwc",
            dir.path().join("proksi.db").to_string_lossy()
        );
        certificates_and_challenges(&store, || SqlStore::connect(&url).unwrap()).await;
    }

    #[cfg(feature = "postgres")]
    #[tokio::test]
    async fn test_postgres_routes_contract() {
        if let Some(store) = postgres_store("proksi_test_routes") {
            contract_tests::routes_contract(&store).await;
        }
    }

    #[cfg(feature = "postgres")]
    #[tokio::test]
    async fn test_postgres_replicas_contract() {
        if let Some(store) = postgres_store("proksi_test_replicas") {
            contract_tests::replicas_contract(&store).await;
        }
    }

    #[cfg(feature = "sqlite")]
    #[tokio::test]
    async fn test_sqlite_leases_contract() {
        let (store, _dir) = sqlite_store();
        contract_tests::leases_contract(&store).await;
    }

    #[cfg(feature = "sqlite")]
    #[tokio::test]
    async fn test_sqlite_certificates_contract() {
        let (store, dir) = sqlite_store();
        let url = format!(
            "sqlite://{}?mode=rwc",
            dir.path().join("proksi.db").to_string_lossy()
        );
        let other = SqlStore::connect(&url).unwrap();
        contract_tests::certificates_contract(&store, &other).await;
    }

    #[cfg(feature = "postgres")]
    #[tokio::test]
    async fn test_postgres_leases_contract() {
        if let Some(store) = postgres_store("proksi_test_leases") {
            contract_tests::leases_contract(&store).await;
        }
    }

    #[cfg(feature = "postgres")]
    #[tokio::test]
    async fn test_postgres_certificates_contract() {
        if let Some(store) = postgres_store("proksi_test_certificates") {
            let url = std::env::var("PROKSI_TEST_POSTGRES_URL").unwrap();
            let other = SqlStore::connect(&postgres_url(&url, "proksi_test_certificates")).unwrap();
            contract_tests::certificates_contract(&store, &other).await;
        }
    }
}
