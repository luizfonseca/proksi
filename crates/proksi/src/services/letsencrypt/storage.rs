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
    pub fn get_persist(&self) -> PersistType {
        match self.config.store.store_type {
            crate::config::StoreType::Redis => {
                let url = self
                    .config
                    .store
                    .redis_url
                    .as_deref()
                    .unwrap_or("redis://localhost:6379");
                PersistType::Redis(RedisPersist::new(url))
            }
            crate::config::StoreType::Memory => {
                // Get directory based on whether we are running on staging/production
                // LetsEncrypt configurations
                let certificates_dir = self.get_lets_encrypt_directory();
                // Ensures the directories exist before we start creating certificates
                tracing::info!(
                    "creating certificates in folder {}",
                    certificates_dir.to_string_lossy()
                );
                create_dir_all(certificates_dir).expect("failed to create directory {certificates_dir:?}. Check permissions or make sure that the parent directory exists beforehand.");

                PersistType::File(acme_v2::persist::FilePersist::new(
                    self.get_lets_encrypt_directory(),
                ))
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
    pub fn new(redis_url: &str) -> Self {
        Self {
            client: redis::Client::open(redis_url).expect("Failed to create client to Redis"),
        }
    }
}

impl acme_v2::persist::Persist for RedisPersist {
    fn get(&self, key: &acme_v2::persist::PersistKey) -> acme_v2::Result<Option<Vec<u8>>> {
        let mut conn = self
            .client
            .get_connection()
            .expect("Failed to get Redis connection");

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
            .expect("Failed to get Redis connection");

        conn.set::<String, &[u8], String>(key.to_string(), value)
            .map_err(|e| acme_v2::Error::Other(e.to_string()))?;

        Ok(())
    }
}
