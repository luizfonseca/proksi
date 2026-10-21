use std::{
    hash::RandomState,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
};

use host_container::HostContainer;
use once_cell::sync::Lazy;
use papaya::HashMapRef;
use routes::RouteStore;

pub mod cache;
pub mod global;
pub mod host_container;
pub mod lookup;
pub mod routes;
pub mod stored_route;

// CHALLENGE store - DEPRECATED: Use global store instead
// static CHALLENGE_STORE: Lazy<ChallengeStore> = Lazy::new(papaya::HashMap::new);

// pub fn get_challenge_by_key(key: &str) -> Option<(String, String)> {
//     CHALLENGE_STORE.pin().get(key).cloned()
// }

// /// Insert given challenge into the store
// pub fn insert_challenge(key: String, value: (String, String)) {
//     CHALLENGE_STORE.pin().insert(key, value);
// }

/// The id of this replica in the store (heartbeats and leases): its hostname and a random
/// suffix, so replicas on one host (or restarts) are told apart
pub fn replica_id() -> &'static str {
    static REPLICA_ID: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    REPLICA_ID.get_or_init(|| {
        let suffix = uuid::Uuid::new_v4().simple().to_string();
        format!("{}-{}", hostname(), suffix.get(..8).unwrap_or(&suffix))
    })
}

pub fn hostname() -> String {
    nix::unistd::gethostname()
        .ok()
        .and_then(|name| name.into_string().ok())
        .unwrap_or_else(|| "proksi".to_string())
}

// ROUTE store
static ROUTE_STORE: Lazy<RouteStore> = Lazy::new(papaya::HashMap::new);

pub fn get_route_by_key(key: &str) -> Option<Arc<HostContainer>> {
    ROUTE_STORE.pin().get(key).cloned()
}

pub fn get_routes(
) -> HashMapRef<'static, String, Arc<HostContainer>, RandomState, seize::OwnedGuard<'static>> {
    ROUTE_STORE.pin_owned()
}

/// Hosts with a wildcard name (`*.example.com`)
static WILDCARD_HOSTS: AtomicUsize = AtomicUsize::new(0);

pub fn insert_route(key: String, value: Arc<HostContainer>) {
    let wildcard = key.starts_with("*.");
    if ROUTE_STORE.pin().insert(key, value).is_none() && wildcard {
        WILDCARD_HOSTS.fetch_add(1, Ordering::Relaxed);
    }
}

pub fn remove_route(key: &str) {
    if ROUTE_STORE.pin().remove(key).is_some() && key.starts_with("*.") {
        WILDCARD_HOSTS.fetch_sub(1, Ordering::Relaxed);
    }
}

/// Whether any host has a wildcard name: requests don't look for one otherwise
pub fn has_wildcard_hosts() -> bool {
    WILDCARD_HOSTS.load(Ordering::Relaxed) > 0
}

// CERTIFICATE store
// static CERTIFICATE_STORE: Lazy<CertificateStore> = Lazy::new(papaya::HashMap::new);

// pub fn get_certificate_by_key(key: &str) -> Option<Certificate> {
//     CERTIFICATE_STORE.pin().get(key).cloned()
// }

// pub fn get_certificates(
// ) -> HashMapRef<'static, String, Certificate, RandomState, seize::LocalGuard<'static>> {
//     CERTIFICATE_STORE.pin()
// }

// pub fn insert_certificate(key: String, value: Certificate) {
//     CERTIFICATE_STORE.pin().insert(key, value);
// }

// Cache Routing store
static CACHE_ROUTING_STORE: Lazy<cache::PathCacheStorage> = Lazy::new(papaya::HashMap::new);

/// Retrieves the cache routing from the store
pub fn get_cache_routing_by_key(key: &str) -> Option<String> {
    CACHE_ROUTING_STORE.pin().get(key).cloned()
}

/// Where the cache of `key` is stored, unless it's known already
pub fn insert_cache_routing(key: &str, value: &str) {
    let store = CACHE_ROUTING_STORE.pin();
    if store.get(key).is_none() {
        store.insert(key.to_string(), value.to_string());
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use crate::routing::{build::build_host_container, test_utils::host};

    #[test]
    fn test_wildcard_hosts_are_counted() {
        let container =
            || Arc::new(build_host_container(&host("*.count.test", "10.0.0.7")).unwrap());
        assert!(!super::has_wildcard_hosts());

        super::insert_route("*.count.test".to_string(), container());
        // Replaced, not added
        super::insert_route("*.count.test".to_string(), container());
        super::insert_route("count.test".to_string(), container());
        assert!(super::has_wildcard_hosts());

        super::remove_route("*.count.test");
        super::remove_route("*.count.test");
        super::remove_route("count.test");
        assert!(!super::has_wildcard_hosts());
    }
}
