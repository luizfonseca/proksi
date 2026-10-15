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
    File(acme_v2::persist::FilePersist),
}

// This ensures that any match logic returns the exact same type (impl Persist)
impl acme_v2::persist::Persist for PersistType {
    fn get(&self, key: &acme_v2::persist::PersistKey) -> acme_v2::Result<Option<Vec<u8>>> {
        match self {
            PersistType::Redis(p) => p.get(key),
            PersistType::File(p) => p.get(key),
        }
    }

    fn put(&self, key: &acme_v2::persist::PersistKey, value: &[u8]) -> acme_v2::Result<()> {
        match self {
            PersistType::Redis(p) => p.put(key, value),
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
            // ACME account data is kept on disk for SQL stores (certificates are in the store)
            crate::config::StoreType::Memory
            | crate::config::StoreType::Sqlite
            | crate::config::StoreType::Postgres => {
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
