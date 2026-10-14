//! Routes come from several sources (configuration file, docker, store...).
//! Each source sends its complete list of routes, and the reconciler merges them
//! into the route table used by the proxy. See `docs/adrs/0001-route-sources.md`.

use serde::Serialize;

use crate::config::Route;

pub mod build;
pub mod merge;
pub mod reconciler;
pub mod validate;

/// Where a route comes from. The order is the priority when several sources
/// define the same host: the last one wins (store > kubernetes > docker > file).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
pub enum RouteSourceKind {
    File,
    Docker,
    Kubernetes,
    Store,
}

/// The complete list of routes of a source
#[derive(Debug, Clone)]
pub struct RouteSnapshot {
    pub source: RouteSourceKind,
    pub routes: Vec<Route>,
    /// Revision of the routes, for sources that have one (the store)
    pub revision: Option<u64>,
}

impl RouteSnapshot {
    pub fn new(source: RouteSourceKind, routes: Vec<Route>) -> Self {
        Self {
            source,
            routes,
            revision: None,
        }
    }
}

pub type SnapshotSender = tokio::sync::mpsc::Sender<RouteSnapshot>;

#[cfg(test)]
pub(crate) mod test_utils {
    use crate::config::Route;

    /// Route for `host` with a single upstream
    pub fn route(host: &str, upstream_ip: &str) -> Route {
        serde_json::from_value(serde_json::json!({
            "host": host,
            "upstreams": [{ "ip": upstream_ip, "port": 3000 }],
        }))
        .unwrap()
    }
}
