//! In-memory copy of disk cache entries, so that cache hits don't need to read from disk.
//!
//! The total size is limited by `cache.memory_limit_mb`. When the limit is reached,
//! the least popular entries are evicted (they're still served from disk).

use std::sync::Arc;

use once_cell::sync::OnceCell;
use tinyufo::TinyUfo;

use super::meta::DiskCacheItemMetadata;

/// Cached entry (metadata and body)
pub type MemoryCacheItem = Arc<(DiskCacheItemMetadata, bytes::Bytes)>;

/// Default memory limit, used when the limit isn't configured
pub const DEFAULT_MEMORY_LIMIT_MB: usize = 256;

/// Entries are weighted by their size in KB, so a single entry can't be larger than this.
/// Larger entries are only served from disk.
const MAX_ENTRY_SIZE_KB: usize = u16::MAX as usize;

pub struct MemoryCache {
    cache: TinyUfo<String, MemoryCacheItem>,
    limit_kb: usize,
}

impl MemoryCache {
    pub fn new(memory_limit_mb: usize) -> Self {
        let limit_kb = memory_limit_mb.saturating_mul(1024);
        Self {
            // Assumes entries are 8KB on average to size the internal structures
            cache: TinyUfo::new(limit_kb, (limit_kb / 8).max(1024)),
            limit_kb,
        }
    }

    pub fn get(&self, key: &String) -> Option<MemoryCacheItem> {
        self.cache.get(key)
    }

    /// Adds the entry, unless it's too large to be kept in memory
    pub fn put(&self, key: String, item: MemoryCacheItem) {
        let size_kb = item.1.len().div_ceil(1024).max(1);
        if size_kb > MAX_ENTRY_SIZE_KB || size_kb > self.limit_kb {
            return;
        }
        let Ok(weight) = u16::try_from(size_kb) else {
            return;
        };

        self.cache.put(key, item, weight);
    }
}

/// The memory cache, created when first used: proksi only sets its limit when it starts, as
/// it's sized for the whole limit (about 2 MB for the default 256 MB) and most configurations
/// don't cache
pub struct LazyMemoryCache {
    limit_mb: OnceCell<usize>,
    cache: OnceCell<Option<MemoryCache>>,
}

impl LazyMemoryCache {
    pub const fn new() -> Self {
        Self {
            limit_mb: OnceCell::new(),
            cache: OnceCell::new(),
        }
    }

    /// Sets the memory limit (in MB). A limit of 0 disables the in-memory copy.
    pub fn set_limit(&self, memory_limit_mb: usize) {
        if self.limit_mb.set(memory_limit_mb).is_err() {
            tracing::warn!("memory cache limit already set");
        }
    }

    /// The cache, or `None` if it's disabled
    pub fn get(&self) -> Option<&MemoryCache> {
        self.cache
            .get_or_init(|| {
                let limit_mb = self
                    .limit_mb
                    .get()
                    .copied()
                    .unwrap_or(DEFAULT_MEMORY_LIMIT_MB);
                (limit_mb > 0).then(|| MemoryCache::new(limit_mb))
            })
            .as_ref()
    }

    #[cfg(test)]
    fn created(&self) -> bool {
        self.cache.get().is_some()
    }
}

static MEMORY_CACHE: LazyMemoryCache = LazyMemoryCache::new();

/// Sets the memory limit (in MB). A limit of 0 disables the in-memory copy.
pub fn init_memory_cache(memory_limit_mb: usize) {
    MEMORY_CACHE.set_limit(memory_limit_mb);
}

/// Returns the memory cache, or `None` if it's disabled
pub fn memory_cache() -> Option<&'static MemoryCache> {
    MEMORY_CACHE.get()
}

#[cfg(test)]
mod tests {
    use std::{sync::Arc, time::SystemTime};

    use super::{DiskCacheItemMetadata, MemoryCache};

    fn item(size: usize) -> super::MemoryCacheItem {
        let meta = DiskCacheItemMetadata {
            status: 200,
            created_at: SystemTime::now(),
            fresh_until: SystemTime::now(),
            stale_while_revalidate_sec: 0,
            stale_if_error_sec: 0,
            headers: std::collections::BTreeMap::new(),
        };
        Arc::new((meta, bytes::Bytes::from(vec![b'x'; size])))
    }

    #[test]
    fn test_memory_cache_is_created_when_first_used() {
        let lazy = super::LazyMemoryCache::new();
        lazy.set_limit(1);
        assert!(!lazy.created());

        assert!(lazy.get().is_some());
        assert!(lazy.created());

        let disabled = super::LazyMemoryCache::new();
        disabled.set_limit(0);
        assert!(disabled.get().is_none());
    }

    #[test]
    fn test_memory_cache_respects_limit() {
        let cache = MemoryCache::new(1);

        // 100 entries of 100KB (~10MB) in a 1MB cache
        for i in 0..100 {
            cache.put(format!("key-{i}"), item(100 * 1024));
        }

        let cached = (0..100)
            .filter(|i| cache.get(&format!("key-{i}")).is_some())
            .count();
        assert!(cached > 0);
        assert!(cached * 100 <= 1024, "{cached} entries of 100KB cached");
    }

    #[test]
    fn test_memory_cache_skips_entries_larger_than_the_limit() {
        let cache = MemoryCache::new(1);

        cache.put("small".to_string(), item(10 * 1024));
        cache.put("large".to_string(), item(2 * 1024 * 1024));

        assert!(cache.get(&"small".to_string()).is_some());
        assert!(cache.get(&"large".to_string()).is_none());
    }
}
