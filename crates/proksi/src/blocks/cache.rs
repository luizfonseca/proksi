//! `cache { ... }`: serves responses from memory or disk

use std::time::Duration;

use anyhow::Result;
use async_trait::async_trait;
use once_cell::sync::Lazy;
use pingora::proxy::Session;
use pingora_cache::lock::CacheLock;

use crate::{
    cache::disk::storage::DiskCache,
    config::{RouteCache, RouteCacheType},
    proxy_server::https_proxy::RouterContext,
    stores,
};

use super::Middleware;

static STORAGE_MEM_CACHE: Lazy<pingora_cache::MemCache> = Lazy::new(pingora_cache::MemCache::new);
static STORAGE_CACHE: Lazy<DiskCache> = Lazy::new(DiskCache::new);
static CACHE_LOCK: Lazy<CacheLock> = Lazy::new(|| CacheLock::new(Duration::from_secs(1)));

fn storage(cache_type: &RouteCacheType) -> &'static (dyn pingora_cache::Storage + Sync) {
    match cache_type {
        RouteCacheType::Disk => &*STORAGE_CACHE,
        RouteCacheType::MemCache => &*STORAGE_MEM_CACHE,
    }
}

pub(super) struct Cache {
    cache: RouteCache,
}

impl Cache {
    /// `None` when the cache isn't enabled
    pub(super) fn new(cache: &RouteCache) -> Option<Self> {
        cache.enabled.unwrap_or(false).then(|| Self {
            cache: cache.clone(),
        })
    }
}

#[async_trait]
impl Middleware for Cache {
    async fn request(&self, session: &mut Session, ctx: &mut RouterContext) -> Result<bool> {
        stores::insert_cache_routing(
            &ctx.host,
            self.cache.path.to_string_lossy().to_string(),
            false,
        );
        session.cache.enable(
            storage(&self.cache.cache_type),
            None,
            None,
            Some(&*CACHE_LOCK),
            None,
        );
        Ok(false)
    }
}
