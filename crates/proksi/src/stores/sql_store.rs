//! SQL store (sqlite and postgres), through sqlx's `Any` driver

use std::{error::Error, future::Future, hash::RandomState, time::Duration};

use async_trait::async_trait;
use papaya::HashMapRef;
use sqlx::{any::AnyPoolOptions, AnyPool, Row};

use super::{
    certificates::{Certificate, SerializableCertificate},
    store_trait::Store,
    stored_route::{decode_stored_route, ReplicaStatus, RoutesSnapshot, StoreError, StoredRoute},
};

const CHALLENGE_TTL_SECONDS: i64 = 300;
const ROUTES_READ_ATTEMPTS: usize = 3;

pub struct SqlStore {
    pool: AnyPool,
    /// Queries run on this runtime: proksi uses several runtimes, and connections
    /// belong to the runtime they were created on
    runtime: Option<tokio::runtime::Runtime>,
    /// Certificates already loaded (also returned by `get_certificates`)
    cache: papaya::HashMap<String, Certificate>,
}

impl SqlStore {
    /// Connects to the database and creates/updates the tables.
    /// For sqlite, use `sqlite://<path>?mode=rwc` to create the file if needed.
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

        Ok(Self {
            pool,
            runtime: Some(runtime),
            cache: papaya::HashMap::new(),
        })
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

async fn increment_revision<'e, E>(executor: E) -> Result<u64, StoreError>
where
    E: sqlx::Executor<'e, Database = sqlx::Any>,
{
    let row = sqlx::query(
        "UPDATE proksi_meta SET value = value + 1 WHERE key = 'revision' RETURNING value",
    )
    .fetch_one(executor)
    .await
    .map_err(unavailable)?;
    to_revision(row.try_get("value").map_err(invalid)?)
}

#[async_trait]
impl Store for SqlStore {
    async fn routes(&self) -> Result<RoutesSnapshot, StoreError> {
        let pool = self.pool.clone();
        self.run(async move {
            // Retried if the routes change while being read
            for _ in 0..ROUTES_READ_ATTEMPTS {
                let current = revision(&pool).await?;
                let rows = sqlx::query("SELECT host, data FROM proksi_routes")
                    .fetch_all(&pool)
                    .await
                    .map_err(unavailable)?;
                if revision(&pool).await? != current {
                    continue;
                }

                let mut routes = Vec::with_capacity(rows.len());
                for row in rows {
                    let host: String = row.try_get("host").map_err(invalid)?;
                    let data: String = row.try_get("data").map_err(invalid)?;
                    routes.push(decode_stored_route(&host, &data));
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

    async fn put_route(&self, route: StoredRoute) -> Result<u64, StoreError> {
        let pool = self.pool.clone();
        let host = route.host();
        let data = serde_json::to_string(&route).map_err(invalid)?;
        let schema_version = i64::from(route.schema_version);
        self.run(async move {
            let mut tx = pool.begin().await.map_err(unavailable)?;
            sqlx::query(
                "INSERT INTO proksi_routes (host, schema_version, data, updated_at) \
                 VALUES ($1, $2, $3, $4) \
                 ON CONFLICT (host) DO UPDATE SET schema_version = excluded.schema_version, \
                 data = excluded.data, updated_at = excluded.updated_at",
            )
            .bind(host)
            .bind(schema_version)
            .bind(data)
            .bind(unix_timestamp())
            .execute(&mut *tx)
            .await
            .map_err(unavailable)?;
            let revision = increment_revision(&mut *tx).await?;
            tx.commit().await.map_err(unavailable)?;
            Ok(revision)
        })
        .await
    }

    async fn delete_route(&self, host: &str) -> Result<u64, StoreError> {
        let pool = self.pool.clone();
        let host = host.to_lowercase();
        self.run(async move {
            let mut tx = pool.begin().await.map_err(unavailable)?;
            let deleted = sqlx::query("DELETE FROM proksi_routes WHERE host = $1")
                .bind(host)
                .execute(&mut *tx)
                .await
                .map_err(unavailable)?
                .rows_affected();
            // The revision only changes when a route was deleted
            let revision = if deleted > 0 {
                increment_revision(&mut *tx).await?
            } else {
                revision(&mut *tx).await?
            };
            tx.commit().await.map_err(unavailable)?;
            Ok(revision)
        })
        .await
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
            sqlx::query(
                "INSERT INTO proksi_certificates (domain, data) VALUES ($1, $2) \
                 ON CONFLICT (domain) DO UPDATE SET data = excluded.data",
            )
            .bind(key)
            .bind(data)
            .execute(&pool)
            .await
            .map_err(unavailable)?;
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
                for row in rows {
                    let (Ok(domain), Ok(data)) = (
                        row.try_get::<String, _>("domain"),
                        row.try_get::<String, _>("data"),
                    ) else {
                        continue;
                    };
                    let certificate = serde_json::from_str::<SerializableCertificate>(&data)
                        .ok()
                        .and_then(|cert| Certificate::from_serializable(cert).ok());
                    if let Some(certificate) = certificate {
                        cache.insert(domain, certificate);
                    }
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
    use crate::stores::{certificates::Certificate, contract_tests, store_trait::Store};

    use super::SqlStore;

    #[cfg(feature = "store-sqlite")]
    fn sqlite_store() -> (SqlStore, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let url = format!(
            "sqlite://{}?mode=rwc",
            dir.path().join("proksi.db").to_string_lossy()
        );
        (SqlStore::connect(&url).unwrap(), dir)
    }

    /// Postgres set in `PROKSI_TEST_POSTGRES_URL`, with a dedicated, emptied schema
    #[cfg(feature = "store-postgres")]
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
        let separator = if url.contains('?') { '&' } else { '?' };
        Some(
            SqlStore::connect(&format!("{url}{separator}options=-c search_path={schema}")).unwrap(),
        )
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
            chain: None,
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

    #[cfg(feature = "store-sqlite")]
    #[tokio::test]
    async fn test_sqlite_routes_contract() {
        let (store, _dir) = sqlite_store();
        contract_tests::routes_contract(&store).await;
    }

    #[cfg(feature = "store-sqlite")]
    #[tokio::test]
    async fn test_sqlite_replicas_contract() {
        let (store, _dir) = sqlite_store();
        contract_tests::replicas_contract(&store).await;
    }

    #[cfg(feature = "store-sqlite")]
    #[tokio::test]
    async fn test_sqlite_certificates_and_challenges() {
        let (store, dir) = sqlite_store();
        let url = format!(
            "sqlite://{}?mode=rwc",
            dir.path().join("proksi.db").to_string_lossy()
        );
        certificates_and_challenges(&store, || SqlStore::connect(&url).unwrap()).await;
    }

    #[cfg(feature = "store-postgres")]
    #[tokio::test]
    async fn test_postgres_routes_contract() {
        if let Some(store) = postgres_store("proksi_test_routes") {
            contract_tests::routes_contract(&store).await;
        }
    }

    #[cfg(feature = "store-postgres")]
    #[tokio::test]
    async fn test_postgres_replicas_contract() {
        if let Some(store) = postgres_store("proksi_test_replicas") {
            contract_tests::replicas_contract(&store).await;
        }
    }
}
