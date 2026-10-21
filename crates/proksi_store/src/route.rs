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
    /// Read from the store: the active version, the revision this definition was saved at
    /// (0 if saved before the store kept versions), which a change can expect
    /// ([`Expect::Version`])
    pub version: u64,
}

impl RouteRecord {
    /// The record of `host` (any case: hosts are stored lowercase), to write
    #[must_use]
    pub fn new(host: &str, schema_version: u32, data: String) -> Self {
        Self {
            host: host.to_lowercase(),
            schema_version,
            data,
            version: 0,
        }
    }
}

/// The versions of a host kept by default (`with_history`)
pub const DEFAULT_HISTORY: usize = 50;

/// What a change to a host expects of it, so two people can't overwrite each other's changes
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Expect {
    /// Nothing: the change applies whatever the host is
    #[default]
    Any,
    /// The host isn't in the store (it never was, or it was deleted)
    Absent,
    /// The host's active version is this one ([`RouteRecord::version`]): the change was
    /// made from it
    Version(u64),
}

/// Who makes a change, and what it expects of the host
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Write {
    pub author: Option<String>,
    pub expect: Expect,
}

/// A saved definition of a host
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RouteVersion {
    /// The definition, with `version` set to the revision it was saved at
    pub record: RouteRecord,
    /// Unix timestamp (seconds)
    pub at: u64,
    pub author: Option<String>,
}

/// What a change did to a host
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "change", rename_all = "snake_case")]
pub enum Change {
    /// A new definition, saved as the version of this revision
    Saved,
    /// The host was removed (its versions are kept)
    Deleted,
    /// The host was set back to the version saved at `to`
    RolledBack { to: u64 },
}

/// A change to a host, newest first in its history
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HistoryEntry {
    pub revision: u64,
    /// Unix timestamp (seconds)
    pub at: u64,
    pub author: Option<String>,
    #[serde(flatten)]
    pub change: Change,
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
    /// The host isn't as the change expected ([`Expect`]): its active version is `version`
    /// (0 if it isn't in the store)
    Conflict { version: u64 },
    /// There's no such version of the host (it was never saved, or is too old to be kept)
    NoVersion(u64),
}

impl fmt::Display for StoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            StoreError::Unavailable(err) => write!(f, "store unavailable: {err}"),
            StoreError::Invalid(err) => write!(f, "invalid data: {err}"),
            StoreError::Conflict { version: 0 } => write!(f, "the host isn't in the store"),
            StoreError::Conflict { version } => {
                write!(f, "the host was changed by someone else: it's on version {version}")
            }
            StoreError::NoVersion(version) => write!(f, "no version {version} of the host"),
        }
    }
}

impl std::error::Error for StoreError {}
