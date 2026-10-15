use async_trait::async_trait;
use papaya::HashMapRef;
use std::{error::Error, hash::RandomState, time::Duration};

use super::{
    certificates::Certificate,
    stored_route::{ReplicaStatus, RoutesSnapshot, StoreError, StoredRoute},
};

// `async_trait` marks the generated methods as `#[must_use]`
#[allow(clippy::double_must_use)]
#[async_trait]
pub trait Store: Send + Sync + 'static {
    /// All stored routes, with the revision they belong to
    async fn routes(&self) -> Result<RoutesSnapshot, StoreError>;

    /// Revision of the routes, incremented by every change
    async fn revision(&self) -> Result<u64, StoreError>;

    /// Adds or replaces the route of a host, returning the new revision
    async fn put_route(&self, route: StoredRoute) -> Result<u64, StoreError>;

    /// Removes the route of a host, returning the (possibly unchanged) revision
    async fn delete_route(&self, host: &str) -> Result<u64, StoreError>;

    /// Records the revision applied by a replica, expiring after `ttl`
    async fn report_replica(&self, status: ReplicaStatus, ttl: Duration) -> Result<(), StoreError>;

    /// Replicas that reported recently
    async fn replicas(&self) -> Result<Vec<ReplicaStatus>, StoreError>;

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
