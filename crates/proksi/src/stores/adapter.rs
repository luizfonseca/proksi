// Adapter module for managing different types of store adapters.
// These are used to abstract where we store certificates, routing information etc.
// For certificates it might be a file system, a database, or a cloud storage service.
// The system will try to sync the information to ensure that a given server is not
// constantly requesting the information and adding latency to requests.

use async_trait::async_trait;
use papaya::HashMapRef;
use std::{error::Error, hash::RandomState, sync::Arc};
use redis::{AsyncCommands, Client, aio::Connection};
use serde_json;
use tokio::sync::Mutex;

use super::certificates::{Certificate, SerializableCertificate};

#[async_trait]
pub trait Store: Send + Sync + 'static {
    // async fn get_route(&self, host: &str) -> Result<Option<String>, Box<dyn Error>>;
    // async fn remove_route(&self, host: &str) -> Result<(), Box<dyn Error>>;
    // async fn set_route(&self, route: &str) -> Result<(), Box<dyn Error>>;
    async fn get_certificate(&self, domain: &str) -> Option<Certificate>;
    async fn set_certificate(&self, domain: &str, cert: Certificate) -> Result<(), Box<dyn Error>>;
    async fn get_certificates(
        &self,
    ) -> HashMapRef<'_, String, Certificate, RandomState, seize::LocalGuard<'_>>;
}

pub struct MemoryStore {
    /// Map of domain names to certificates (including leaf & chain)
    inner_certs: papaya::HashMap<String, Certificate>,
}

impl MemoryStore {
    pub fn new() -> Self {
        MemoryStore {
            inner_certs: papaya::HashMap::new(),
        }
    }
}

#[async_trait]
impl Store for MemoryStore {
    async fn get_certificate(&self, host: &str) -> Option<Certificate> {
        self.inner_certs.pin().get(host).cloned()
    }

    async fn set_certificate(&self, host: &str, cert: Certificate) -> Result<(), Box<dyn Error>> {
        self.inner_certs.pin().insert(host.to_string(), cert);
        Ok(())
    }

    async fn get_certificates(
        &self,
    ) -> HashMapRef<'_, String, Certificate, RandomState, seize::LocalGuard<'_>> {
        self.inner_certs.pin()
    }
}

pub struct RedisStore {
    connection: Arc<Mutex<Connection>>,
    cache: Arc<dashmap::DashMap<String, Certificate>>,
}

impl RedisStore {
    pub async fn new(redis_url: &str) -> Result<Self, Box<dyn Error>> {
        let client = Client::open(redis_url)?;
        let connection = client.get_async_connection().await?;
        
        Ok(RedisStore {
            connection: Arc::new(Mutex::new(connection)),
            cache: Arc::new(dashmap::DashMap::new()),
        })
    }

    fn certificate_key(domain: &str) -> String {
        format!("proksi:cert:{}", domain)
    }

    async fn load_from_redis(&self, domain: &str) -> Option<Certificate> {
        let mut conn = self.connection.lock().await;
        let key = Self::certificate_key(domain);
        
        let cert_data: Option<String> = conn.get(&key).await.ok()?;
        
        if let Some(data) = cert_data {
            if let Ok(serializable_cert) = serde_json::from_str::<SerializableCertificate>(&data) {
                return Certificate::from_serializable(serializable_cert).ok();
            }
        }
        None
    }
}

#[async_trait]
impl Store for RedisStore {
    async fn get_certificate(&self, domain: &str) -> Option<Certificate> {
        // Check cache first
        if let Some(cert) = self.cache.get(domain) {
            return Some(cert.clone());
        }

        // If not in cache, load from Redis
        if let Some(cert) = self.load_from_redis(domain).await {
            // Store in cache for future use
            self.cache.insert(domain.to_string(), cert.clone());
            return Some(cert);
        }

        None
    }

    async fn set_certificate(&self, domain: &str, cert: Certificate) -> Result<(), Box<dyn Error>> {
        let mut conn = self.connection.lock().await;
        let key = Self::certificate_key(domain);
        
        // Update Redis
        let serializable_cert = cert.to_serializable()?;
        let cert_json = serde_json::to_string(&serializable_cert)?;
        conn.set(&key, &cert_json).await?;

        // Update cache
        self.cache.insert(domain.to_string(), cert);
        
        Ok(())
    }

    async fn get_certificates(&self) -> HashMapRef<'_, String, Certificate, RandomState, seize::LocalGuard<'_>> {
        static TEMP_MAP: once_cell::sync::Lazy<papaya::HashMap<String, Certificate>> = 
            once_cell::sync::Lazy::new(papaya::HashMap::new);
        
        TEMP_MAP.pin().clear();
        
        // First, copy all cached certificates
        for entry in self.cache.iter() {
            TEMP_MAP.pin().insert(entry.key().clone(), entry.value().clone());
        }
        
        // Then get any additional certificates from Redis
        let mut conn_guard = self.connection.lock().await;
        let pattern = "proksi:cert:*".to_string();
        if let Ok(keys) = conn_guard.keys::<_, Vec<String>>(&pattern).await {
            for key in keys {
                if let Some(domain) = key.strip_prefix("proksi:cert:") {
                    let domain = domain.to_string();
                    // Skip if we already have it from cache
                    if TEMP_MAP.pin().contains_key(&domain) {
                        continue;
                    }
                    
                    if let Ok(cert_data) = conn_guard.get::<_, String>(&key).await {
                        if let Ok(serializable_cert) = serde_json::from_str::<SerializableCertificate>(&cert_data) {
                            if let Ok(cert) = Certificate::from_serializable(serializable_cert) {
                                TEMP_MAP.pin().insert(domain.clone(), cert.clone());
                                // Update cache with newly found certificate
                                self.cache.insert(domain, cert);
                            }
                        }
                    }
                }
            }
        }
        
        TEMP_MAP.pin()
    }
}
