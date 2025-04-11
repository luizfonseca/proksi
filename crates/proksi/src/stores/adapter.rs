// Adapter module for managing different types of store adapters.
// These are used to abstract where we store certificates, routing information etc.
// For certificates it might be a file system, a database, or a cloud storage service.
// The system will try to sync the information to ensure that a given server is not
// constantly requesting the information and adding latency to requests.

use async_trait::async_trait;
use papaya::HashMapRef;
use std::{error::Error, hash::RandomState};

use super::certificates::Certificate;

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
