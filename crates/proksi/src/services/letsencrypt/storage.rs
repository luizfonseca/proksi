use std::{
    fs::create_dir_all,
    path::{self, PathBuf},
    sync::Arc,
};

use redis::Commands;

/// `PersistType` enum represents the type of persistence used for storing certificates.
#[derive(Clone)]
pub enum PersistType {
    Redis(RedisPersist),
    #[cfg(any(feature = "store-sqlite", feature = "store-postgres"))]
    Sql(SqlPersist),
    File(acme_v2::persist::FilePersist),
}

// This ensures that any match logic returns the exact same type (impl Persist)
impl acme_v2::persist::Persist for PersistType {
    fn get(&self, key: &acme_v2::persist::PersistKey) -> acme_v2::Result<Option<Vec<u8>>> {
        match self {
            PersistType::Redis(p) => p.get(key),
            #[cfg(any(feature = "store-sqlite", feature = "store-postgres"))]
            PersistType::Sql(p) => p.get(key),
            PersistType::File(p) => p.get(key),
        }
    }

    fn put(&self, key: &acme_v2::persist::PersistKey, value: &[u8]) -> acme_v2::Result<()> {
        match self {
            PersistType::Redis(p) => p.put(key, value),
            #[cfg(any(feature = "store-sqlite", feature = "store-postgres"))]
            PersistType::Sql(p) => p.put(key, value),
            PersistType::File(p) => p.put(key, value),
        }
    }
}

pub struct CertificatePersist {
    config: Arc<crate::config::Config>,
}

impl CertificatePersist {
    pub fn new(config: Arc<crate::config::Config>) -> Self {
        Self { config }
    }

    // This ensures that any match logic returns the exact same type (impl Persist)
    pub fn get_persist(&self) -> Result<PersistType, anyhow::Error> {
        match self.config.store.store_type {
            crate::config::StoreType::Redis => {
                let url = self
                    .config
                    .store
                    .redis_url
                    .as_deref()
                    .unwrap_or("redis://localhost:6379");
                Ok(PersistType::Redis(RedisPersist::new(url)?))
            }
            #[cfg(any(feature = "store-sqlite", feature = "store-postgres"))]
            crate::config::StoreType::Sqlite | crate::config::StoreType::Postgres => {
                let url = self.config.store.database_url.as_deref().ok_or_else(|| {
                    anyhow::anyhow!("store.database_url is required for SQL stores")
                })?;
                Ok(PersistType::Sql(SqlPersist::connect(url)?))
            }
            #[cfg(not(any(feature = "store-sqlite", feature = "store-postgres")))]
            crate::config::StoreType::Sqlite | crate::config::StoreType::Postgres => Err(
                anyhow::anyhow!("proksi was built without support for SQL stores"),
            ),
            crate::config::StoreType::Memory => {
                // Get directory based on whether we are running on staging/production
                // LetsEncrypt configurations
                let certificates_dir = self.get_lets_encrypt_directory();
                // Ensures the directories exist before we start creating certificates
                tracing::info!(
                    "creating certificates in folder {}",
                    certificates_dir.to_string_lossy()
                );
                create_dir_all(&certificates_dir).map_err(|err| {
                    anyhow::anyhow!("failed to create directory {certificates_dir:?}: {err}. Check permissions or make sure that the parent directory exists beforehand.")
                })?;

                Ok(PersistType::File(acme_v2::persist::FilePersist::new(
                    certificates_dir,
                )))
            }
        }
    }

    /// Return the appropriate Let's Encrypt directories for certificates based on the environment
    fn get_lets_encrypt_directory(&self) -> PathBuf {
        let suffix = match self.config.lets_encrypt.staging {
            Some(false) => "production",
            _ => "staging",
        };

        let path = self.config.paths.lets_encrypt.join(suffix);

        if let Ok(res) = path::absolute(&path) {
            return res;
        }

        path
    }
}

#[derive(Clone)]
/// `RedisPersist` is a struct that implements the Persist trait for storing and retrieving Let's Encrypt certificates.
pub struct RedisPersist {
    client: redis::Client,
}

impl RedisPersist {
    pub fn new(redis_url: &str) -> redis::RedisResult<Self> {
        Ok(Self {
            client: redis::Client::open(redis_url)?,
        })
    }
}

impl acme_v2::persist::Persist for RedisPersist {
    fn get(&self, key: &acme_v2::persist::PersistKey) -> acme_v2::Result<Option<Vec<u8>>> {
        let mut conn = self
            .client
            .get_connection()
            .map_err(|e| acme_v2::Error::Other(e.to_string()))?;

        if let Ok(value) = conn.get::<String, String>(key.to_string()) {
            if value.is_empty() {
                return Ok(None);
            }

            return Ok(Some(value.into_bytes()));
        }

        Ok(None)
    }

    fn put(&self, key: &acme_v2::persist::PersistKey, value: &[u8]) -> acme_v2::Result<()> {
        let mut conn = self
            .client
            .get_connection()
            .map_err(|e| acme_v2::Error::Other(e.to_string()))?;

        conn.set::<String, &[u8], String>(key.to_string(), value)
            .map_err(|e| acme_v2::Error::Other(e.to_string()))?;

        Ok(())
    }
}

#[cfg(any(feature = "store-sqlite", feature = "store-postgres"))]
#[derive(Clone)]
/// `SqlPersist` stores Let's Encrypt data in the SQL store, so all replicas share the same account
pub struct SqlPersist {
    store: Arc<crate::stores::sql_store::SqlStore>,
}

#[cfg(any(feature = "store-sqlite", feature = "store-postgres"))]
impl SqlPersist {
    pub fn connect(database_url: &str) -> Result<Self, anyhow::Error> {
        let store = crate::stores::sql_store::SqlStore::connect(database_url)
            .map_err(|err| anyhow::anyhow!("failed to connect to the SQL store: {err}"))?;
        Ok(Self {
            store: Arc::new(store),
        })
    }
}

#[cfg(any(feature = "store-sqlite", feature = "store-postgres"))]
impl acme_v2::persist::Persist for SqlPersist {
    fn get(&self, key: &acme_v2::persist::PersistKey) -> acme_v2::Result<Option<Vec<u8>>> {
        self.store
            .acme_get(&key.to_string())
            .map(|value| value.map(String::into_bytes))
            .map_err(|e| acme_v2::Error::Other(e.to_string()))
    }

    fn put(&self, key: &acme_v2::persist::PersistKey, value: &[u8]) -> acme_v2::Result<()> {
        // Keys and certificates are PEM, which is always valid UTF-8
        let value = String::from_utf8(value.to_vec())
            .map_err(|e| acme_v2::Error::Other(format!("invalid value for {key}: {e}")))?;
        self.store
            .acme_put(&key.to_string(), value)
            .map_err(|e| acme_v2::Error::Other(e.to_string()))
    }
}

#[cfg(all(test, feature = "store-sqlite"))]
mod tests {
    use acme_v2::persist::{Persist, PersistKey, PersistKind};

    use super::SqlPersist;

    #[tokio::test]
    async fn test_sql_persist_is_shared_between_replicas() {
        let dir = tempfile::tempdir().unwrap();
        let url = format!(
            "sqlite://{}?mode=rwc",
            dir.path().join("proksi.db").to_string_lossy()
        );
        let replica_a = SqlPersist::connect(&url).unwrap();
        let replica_b = SqlPersist::connect(&url).unwrap();
        let key = PersistKey::new("me@example.com", PersistKind::AccountPrivateKey, "acme");

        assert_eq!(replica_b.get(&key).unwrap(), None);

        replica_a.put(&key, b"first key").unwrap();
        assert_eq!(replica_b.get(&key).unwrap(), Some(b"first key".to_vec()));

        replica_b.put(&key, b"second key").unwrap();
        assert_eq!(replica_a.get(&key).unwrap(), Some(b"second key".to_vec()));
    }
}
