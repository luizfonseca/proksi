//! Routes as the store keeps them, and what replicas report

use std::fmt;

use serde::{Deserialize, Serialize};

/// A host's definition: the key it's stored under (its name, lowercase), the version of the
/// format of its data, and its data, as written by Proksi
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RouteRecord {
    pub host: String,
    pub schema_version: u32,
    pub data: String,
}

impl RouteRecord {
    /// The record of `host` (any case: hosts are stored lowercase)
    #[must_use]
    pub fn new(host: &str, schema_version: u32, data: String) -> Self {
        Self {
            host: host.to_lowercase(),
            schema_version,
            data,
        }
    }
}

/// All routes in the store, at a given revision
#[derive(Debug, Default)]
pub struct RoutesSnapshot {
    pub revision: u64,
    pub routes: Vec<RouteRecord>,
}

/// Reported by each replica after applying the routes of a revision
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ReplicaStatus {
    pub id: String,
    pub revision: u64,
    /// Unix timestamp (seconds)
    pub applied_at: u64,
    pub version: String,
    pub hostname: String,
}

#[derive(Debug, Clone, PartialEq)]
pub enum StoreError {
    /// The store couldn't be reached or failed
    Unavailable(String),
    /// The data is invalid
    Invalid(String),
}

impl fmt::Display for StoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            StoreError::Unavailable(err) => write!(f, "store unavailable: {err}"),
            StoreError::Invalid(err) => write!(f, "invalid data: {err}"),
        }
    }
}

impl std::error::Error for StoreError {}
