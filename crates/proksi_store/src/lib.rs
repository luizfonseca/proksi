//! The stores shared by Proksi replicas (memory, Redis, SQLite, Postgres): routes and their
//! revision, certificates, ACME challenges, leases and replica reports.
//!
//! Routes are kept as records of JSON written by Proksi, which reads them: the store doesn't
//! look inside them.

pub mod certificates;
mod memory_store;
mod redis_store;
mod route;
#[cfg(any(feature = "sqlite", feature = "postgres"))]
mod sql_store;
mod store_trait;

#[cfg(test)]
mod contract_tests;
#[cfg(any(test, feature = "test-util"))]
pub mod test_util;

pub use certificates::{Certificate, SerializableCertificate};
pub use memory_store::MemoryStore;
pub use redis_store::RedisStore;
pub use route::{ReplicaStatus, RouteRecord, RoutesSnapshot, StoreError};
#[cfg(any(feature = "sqlite", feature = "postgres"))]
pub use sql_store::SqlStore;
pub use store_trait::Store;
