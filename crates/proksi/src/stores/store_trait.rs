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

    // Challenge methods for storing ACME challenge tokens and proofs
    async fn get_challenge(&self, domain: &str) -> Option<(String, String)>;
    async fn set_challenge(
        &self,
        domain: &str,
        token: String,
        proof: String,
    ) -> Result<(), Box<dyn Error>>;
}
