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

pub fn get_cache_routing_by_key(key: &str) -> Option<mapref::one::Ref<'static, String, String>> {
    CACHE_ROUTING_STORE.get(key)
}

// pub fn get_cache_routings() -> Guard<Arc<cache::CacheStore>> {
//     CACHE_ROUTING_STORE.load()
// }

pub fn insert_cache_routing(key: String, new_value: String) {
    CACHE_ROUTING_STORE.alter(&key, |_, old_value| {
        if old_value != new_value {
            return new_value;
        }

        old_value
    });
}
