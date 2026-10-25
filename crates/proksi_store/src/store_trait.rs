use async_trait::async_trait;
use papaya::HashMapRef;
use std::{error::Error, hash::RandomState, time::Duration};

use crate::{
    certificates::Certificate,
    notify::{NotifyRecord, NotifySnapshot},
    route::{HistoryEntry, ReplicaStatus, RouteRecord, RouteVersion, RoutesSnapshot, StoreError, Write},
    waiting_room::{Admission, RoomLimits},
};

// `async_trait` marks the generated methods as `#[must_use]`
#[allow(clippy::double_must_use)]
#[async_trait]
pub trait Store: Send + Sync + 'static {
    /// All stored routes, with the revision they belong to
    async fn routes(&self) -> Result<RoutesSnapshot, StoreError>;

    /// Revision of the routes, incremented by every change
    async fn revision(&self) -> Result<u64, StoreError>;

    /// Saves a new version of a host and makes it the active one, returning the new revision.
    /// The store keeps the last versions of each host (`with_history`), and the active one.
    ///
    /// Refused with [`StoreError::Conflict`] when the host isn't as `write` expects.
    async fn put_route(&self, route: RouteRecord, write: &Write) -> Result<u64, StoreError>;

    /// Removes a host, keeping its versions, and returns the new revision (unchanged if it
    /// wasn't in the store and nothing was expected of it)
    async fn delete_route(&self, host: &str, write: &Write) -> Result<u64, StoreError>;

    /// Makes the version of `host` saved at revision `to` its active one again (also for a
    /// deleted host), returning the new revision.
    ///
    /// Refused with [`StoreError::NoVersion`] when that version isn't kept.
    async fn rollback_route(&self, host: &str, to: u64, write: &Write) -> Result<u64, StoreError>;

    /// The changes to a host, newest first (the last ones, as many as versions are kept)
    async fn route_history(&self, host: &str) -> Result<Vec<HistoryEntry>, StoreError>;

    /// The version of `host` saved at revision `version`, if it's kept
    async fn route_version(
        &self,
        host: &str,
        version: u64,
    ) -> Result<Option<RouteVersion>, StoreError>;

    /// Hosts that were deleted and still have versions, which a rollback restores
    async fn deleted_routes(&self) -> Result<Vec<String>, StoreError>;

    /// Revision of the `notify` blocks, incremented by every change to one
    async fn notify_revision(&self) -> Result<u64, StoreError>;

    /// All stored `notify` blocks, with the revision they belong to
    async fn notify_blocks(&self) -> Result<NotifySnapshot, StoreError>;

    /// Saves a `notify` block, returning the new revision of the blocks.
    ///
    /// Refused with [`StoreError::Conflict`] when the block isn't as `write` expects.
    async fn put_notify(&self, block: NotifyRecord, write: &Write) -> Result<u64, StoreError>;

    /// Removes a `notify` block, returning the new revision (unchanged if it wasn't in the
    /// store and nothing was expected of it)
    async fn delete_notify(&self, name: &str, write: &Write) -> Result<u64, StoreError>;

    /// Reports the status of this replica (e.g. the applied revision), expiring after `ttl`
    async fn report(&self, status: ReplicaStatus, ttl: Duration) -> Result<(), StoreError>;

    /// Replicas that reported recently
    #[allow(dead_code)] // used by the admin API (#49)
    async fn replicas(&self) -> Result<Vec<ReplicaStatus>, StoreError>;

    /// Takes the lease `name` for `holder` if nobody else holds it (or `holder` already does),
    /// until it is released or `ttl` passes, by the store's clock
    async fn try_lock(&self, name: &str, holder: &str, ttl: Duration) -> Result<bool, StoreError>;

    /// Releases the lease `name`, if `holder` still holds it
    async fn unlock(&self, name: &str, holder: &str) -> Result<(), StoreError>;

    /// Who holds the lease `name`, if it's held
    async fn lock_holder(&self, name: &str) -> Result<Option<String>, StoreError>;

    /// Lets `visitor` into the waiting room `room` when a slot is free and nobody who came
    /// before waits for it, keeping the slot `limits.session` after this call; otherwise keeps
    /// their place in line. Visitors who held a slot keep it, renewed; waiting visitors who
    /// didn't call for `limits.abandon` lose their place.
    async fn wait_in_room(
        &self,
        room: &str,
        visitor: &str,
        limits: RoomLimits,
    ) -> Result<Admission, StoreError>;

    /// The key of waiting rooms' cookies: random, created on first use, and the same for every
    /// replica sharing the store
    async fn waiting_room_key(&self) -> Result<[u8; 32], StoreError>;

    /// Moves each cursor of `namespace` (by key) forward: to the store's now if it's behind,
    /// plus its step, in microseconds. A cursor expires once the store's now passes it. Answers
    /// how many microseconds ahead of now each one is, in order. Empty when the store doesn't
    /// keep cursors.
    async fn advance_cursors(
        &self,
        namespace: &str,
        steps: &[(String, u64)],
    ) -> Result<Vec<u64>, StoreError>;

    /// Revision of the certificates, incremented by every certificate written
    async fn certificates_revision(&self) -> Result<u64, StoreError>;

    /// Replaces the certificates cached by this replica with the ones in the store
    async fn reload_certificates(&self) -> Result<(), StoreError>;

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
