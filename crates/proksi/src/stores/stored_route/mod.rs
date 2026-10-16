//! Routes kept in a store, shared by every replica

use std::fmt;

use serde::{Deserialize, Serialize};

use crate::config::host::Host;

mod v1;

/// Version of the stored route format written by this version of proksi
/// (1: a route, 2: a host with locations)
pub const ROUTE_SCHEMA_VERSION: u32 = 2;

/// A host as kept in the store
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StoredRoute {
    pub schema_version: u32,
    pub host: Host,
}

impl StoredRoute {
    #[allow(dead_code)] // used by the admin API (#49)
    pub fn new(host: Host) -> Self {
        Self {
            schema_version: ROUTE_SCHEMA_VERSION,
            host,
        }
    }

    /// Key the host is stored under (its lowercase name)
    pub fn key(&self) -> String {
        self.host.name.to_lowercase()
    }
}

/// A stored route that couldn't be read (e.g. invalid data or a newer schema version)
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct StoredRouteError {
    pub host: String,
    pub error: String,
}

/// All routes in the store, at a given revision
#[derive(Debug, Default)]
pub struct RoutesSnapshot {
    pub revision: u64,
    pub routes: Vec<Result<StoredRoute, StoredRouteError>>,
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

/// Reads a stored route, rejecting schema versions this version of proksi doesn't know
pub fn decode_stored_route(host: &str, json: &str) -> Result<StoredRoute, StoredRouteError> {
    let error = |error: String| StoredRouteError {
        host: host.to_string(),
        error,
    };

    // The version is read first: newer versions may not be readable as a route at all
    let version: SchemaVersion =
        serde_json::from_str(json).map_err(|err| error(format!("invalid route: {err}")))?;
    match version.schema_version {
        1 => v1::decode_v1(json)
            .map(StoredRoute::new)
            .map_err(|err| error(format!("invalid route: {err}"))),
        ROUTE_SCHEMA_VERSION => {
            serde_json::from_str(json).map_err(|err| error(format!("invalid route: {err}")))
        }
        version => Err(error(unsupported_version(version))),
    }
}

/// Checks that this version of proksi can read a stored route
pub fn check_schema_version(stored: StoredRoute) -> Result<StoredRoute, StoredRouteError> {
    if stored.schema_version > ROUTE_SCHEMA_VERSION {
        return Err(StoredRouteError {
            host: stored.key(),
            error: unsupported_version(stored.schema_version),
        });
    }
    Ok(stored)
}

#[derive(Deserialize)]
struct SchemaVersion {
    schema_version: u32,
}

fn unsupported_version(version: u32) -> String {
    format!(
        "unsupported schema version {version} (this version of proksi supports up to {ROUTE_SCHEMA_VERSION})"
    )
}

#[cfg(test)]
mod tests {
    use crate::routing::test_utils::host;

    use super::{check_schema_version, decode_stored_route, StoredRoute};

    #[test]
    fn test_decode_stored_route() {
        let stored = StoredRoute::new(host("a.test", "10.0.0.1"));
        let json = serde_json::to_string(&stored).unwrap();

        assert_eq!(decode_stored_route("a.test", &json), Ok(stored));
    }

    #[test]
    fn test_decode_invalid_stored_route() {
        let err = decode_stored_route("a.test", "{ not json").unwrap_err();

        assert_eq!(err.host, "a.test");
        assert!(err.error.starts_with("invalid route"), "{}", err.error);
    }

    #[test]
    fn test_decode_schema_version_1_route_as_host() {
        let json = serde_json::json!({
            "schema_version": 1,
            "route": { "host": "a.test", "upstreams": [{ "ip": "10.0.0.1", "port": 3000 }] },
        })
        .to_string();

        let stored = decode_stored_route("a.test", &json).unwrap();

        assert_eq!(stored.schema_version, 2);
        assert_eq!(stored.host.name, "a.test");
        assert_eq!(stored.host.locations[0].path, "/*");
    }

    #[test]
    fn test_decode_newer_schema_version() {
        let json = serde_json::json!({
            "schema_version": 3,
            "host": { "name": "a.test", "something_new": true },
        })
        .to_string();

        let err = decode_stored_route("a.test", &json).unwrap_err();

        assert_eq!(
            err.error,
            "unsupported schema version 3 (this version of proksi supports up to 2)"
        );
    }

    #[test]
    fn test_check_schema_version() {
        let mut stored = StoredRoute::new(host("a.test", "10.0.0.1"));
        assert!(check_schema_version(stored.clone()).is_ok());

        stored.schema_version = 3;
        assert!(check_schema_version(stored).is_err());
    }

    #[test]
    fn test_stored_route_key_is_lowercase() {
        assert_eq!(StoredRoute::new(host("A.Test", "10.0.0.1")).key(), "a.test");
    }
}
