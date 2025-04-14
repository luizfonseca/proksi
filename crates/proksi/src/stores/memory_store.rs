use async_trait::async_trait;
use papaya::HashMapRef;
use std::{error::Error, hash::RandomState};

use super::certificates::Certificate;
use super::store_trait::Store;

pub struct MemoryStore {
    /// Map of domain names to certificates (including leaf & chain)
    inner_certs: papaya::HashMap<String, Certificate>,
    /// Map of domain names to challenge tokens and proofs (token, proof)
    inner_challenges: papaya::HashMap<String, (String, String)>,
}

impl MemoryStore {
    pub fn new() -> Self {
        MemoryStore {
            inner_certs: papaya::HashMap::new(),
            inner_challenges: papaya::HashMap::new(),
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
