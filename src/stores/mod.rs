use std::{collections::HashMap, sync::Arc};

use arc_swap::{ArcSwap, Guard};
use certificates::{Certificate, CertificateStore};
use challenges::ChallengeStore;
use once_cell::sync::Lazy;
use routes::{RouteStore, RouteStoreContainer};

pub mod cache;
pub mod certificates;
pub mod challenges;
pub mod routes;

// CHALLENGE store
static CHALLENGE_STORE: Lazy<ArcSwap<ChallengeStore>> =
    Lazy::new(|| ArcSwap::new(Arc::new(HashMap::new())));

pub fn get_challenge_by_key(key: &str) -> Option<(String, String)> {
    CHALLENGE_STORE.load().get(key).cloned()
}

/// Insert given challenge into the store
pub fn insert_challenge(key: String, value: (String, String)) {
    let mut map = (**CHALLENGE_STORE.load()).clone();

    map.insert(key, value);

    CHALLENGE_STORE.store(Arc::new(map));
}

// ROUTE store
static ROUTE_STORE: Lazy<ArcSwap<RouteStore>> =
    Lazy::new(|| ArcSwap::new(Arc::new(HashMap::new())));

pub fn get_route_by_key(key: &str) -> Option<RouteStoreContainer> {
    ROUTE_STORE.load().get(key).cloned()
}

pub fn get_routes() -> Guard<Arc<RouteStore>> {
    ROUTE_STORE.load()
}

pub fn _swap_routes(map: RouteStore) {
    ROUTE_STORE.store(Arc::new(map));
}

pub fn insert_route(key: String, value: RouteStoreContainer) {
    let mut map = (**ROUTE_STORE.load()).clone();

    map.insert(key, value);

    ROUTE_STORE.store(Arc::new(map));
}

// CERTIFICATE store
static CERTIFICATE_STORE: Lazy<ArcSwap<CertificateStore>> =
    Lazy::new(|| ArcSwap::new(Arc::new(HashMap::new())));

pub fn get_certificate_by_key(key: &str) -> Option<Certificate> {
    CERTIFICATE_STORE.load().get(key).cloned()
}

pub fn get_certificates() -> Guard<Arc<CertificateStore>> {
    CERTIFICATE_STORE.load()
}

pub fn insert_certificate(key: String, value: Certificate) {
    let mut map = (**CERTIFICATE_STORE.load()).clone();

    map.insert(key, value);

    CERTIFICATE_STORE.store(Arc::new(map));
}

// Cache Routing store
static CACHE_ROUTING_STORE: Lazy<ArcSwap<cache::PathCacheStorage>> =
    Lazy::new(|| ArcSwap::new(Arc::new(HashMap::new())));

pub fn get_cache_routing_by_key(key: &str) -> Option<String> {
    CACHE_ROUTING_STORE.load().get(key).cloned()
}

// pub fn get_cache_routings() -> Guard<Arc<cache::CacheStore>> {
//     CACHE_ROUTING_STORE.load()
// }

pub fn insert_cache_routing(key: String, value: String) {
    let route = CACHE_ROUTING_STORE.load();

    // Dont insert if the key already exists
    if route.get(&key).is_some() {
        return;
    }

    let mut map = (**route).clone();

    map.insert(key, value);

    CACHE_ROUTING_STORE.store(Arc::new(map));
}
