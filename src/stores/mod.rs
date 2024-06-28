use std::{hash::RandomState, sync::Arc};

use certificates::{Certificate, CertificateStore};
use challenges::ChallengeStore;
use dashmap::{mapref, DashMap, ReadOnlyView};
use once_cell::sync::Lazy;
use routes::{RouteStore, RouteStoreContainer};

pub mod cache;
pub mod certificates;
pub mod challenges;
pub mod routes;

// CHALLENGE store
static CHALLENGE_STORE: Lazy<Arc<ChallengeStore>> = Lazy::new(|| Arc::new(DashMap::new()));

pub fn get_challenge_by_key(
    key: &str,
) -> Option<mapref::one::Ref<'static, String, (String, String)>> {
    CHALLENGE_STORE.get(key)
}

/// Insert given challenge into the store
pub fn insert_challenge(key: String, value: (String, String)) {
    CHALLENGE_STORE.insert(key, value);
}

// ROUTE store
static ROUTE_STORE: Lazy<Arc<RouteStore>> = Lazy::new(|| Arc::new(DashMap::new()));

pub fn get_route_by_key(
    key: &str,
) -> Option<mapref::one::Ref<'static, String, RouteStoreContainer>> {
    ROUTE_STORE.get(key)
}

pub fn get_routes() -> ReadOnlyView<String, RouteStoreContainer> {
    (**ROUTE_STORE).clone().into_read_only()
}

pub fn insert_route(key: String, value: RouteStoreContainer) {
    ROUTE_STORE.insert(key, value);
}

pub fn get_mutable_routes(
) -> dashmap::iter::IterMut<'static, String, RouteStoreContainer, RandomState, RouteStore> {
    (*ROUTE_STORE).iter_mut()
}

// CERTIFICATE store
static CERTIFICATE_STORE: Lazy<Arc<CertificateStore>> = Lazy::new(|| Arc::new(DashMap::new()));

pub fn get_certificate_by_key(key: &str) -> Option<mapref::one::Ref<'static, String, Certificate>> {
    CERTIFICATE_STORE.get(key)
}

pub fn get_certificates() -> ReadOnlyView<String, Certificate> {
    (**CERTIFICATE_STORE).clone().into_read_only()
}

pub fn insert_certificate(key: String, value: Certificate) {
    CERTIFICATE_STORE.insert(key, value);
}

// Cache Routing store
static CACHE_ROUTING_STORE: Lazy<Arc<cache::PathCacheStorage>> =
    Lazy::new(|| Arc::new(DashMap::new()));

/// Retrieves the cache routing from the store
pub fn get_cache_routing_by_key(key: &str) -> Option<mapref::one::Ref<'static, String, String>> {
    CACHE_ROUTING_STORE.get(key)
}

/// Insert given cache routing into the store if it does not exist
pub fn insert_cache_routing(key: &str, new_value: String, should_override: bool) {
    if let Some(_) = CACHE_ROUTING_STORE.get(key) {
        if should_override {
            CACHE_ROUTING_STORE.insert(key.to_string(), new_value);

            return;
        }

        return;
    }

    CACHE_ROUTING_STORE.insert(key.to_string(), new_value);
}
