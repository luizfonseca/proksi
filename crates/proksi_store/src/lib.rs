//! The stores shared by Proksi replicas (memory, Redis, SQLite, Postgres): routes and their
//! revision, `notify` blocks, certificates, ACME challenges, leases, replica reports and waiting rooms.
//!
//! Routes are kept as records of JSON written by Proksi, which reads them: the store doesn't
//! look inside them.

pub mod certificates;
mod memory_store;
mod notify;
mod cursors;
mod redis_history;
mod redis_notify;
mod redis_cursors;
mod redis_store;
mod redis_waiting_room;
mod route;
#[cfg(any(feature = "sqlite", feature = "postgres"))]
mod sql_history;
#[cfg(any(feature = "sqlite", feature = "postgres"))]
mod sql_notify;
#[cfg(any(feature = "sqlite", feature = "postgres"))]
mod sql_store;
#[cfg(any(feature = "sqlite", feature = "postgres"))]
mod sql_waiting_room;
mod store_trait;
mod waiting_room;

#[cfg(test)]
mod contract_tests;
#[cfg(any(test, feature = "test-util"))]
pub mod test_util;

pub use certificates::{Certificate, SerializableCertificate};
pub use memory_store::MemoryStore;
pub use notify::{NotifyRecord, NotifySnapshot};
pub use redis_store::RedisStore;
pub use route::{
    Change, Expect, HistoryEntry, ReplicaStatus, RouteRecord, RouteVersion, RoutesSnapshot, StoreError,
    Write, DEFAULT_HISTORY,
};
#[cfg(any(feature = "sqlite", feature = "postgres"))]
pub use sql_store::SqlStore;
pub use store_trait::Store;
pub use waiting_room::{Admission, RoomLimits};
