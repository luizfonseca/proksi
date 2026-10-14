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

static MEMORY_CACHE: OnceCell<Option<MemoryCache>> = OnceCell::new();

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

/// Sets the memory limit (in MB). A limit of 0 disables the in-memory copy.
/// Has no effect if the memory cache was already used.
pub fn init_memory_cache(memory_limit_mb: usize) {
    let cache = (memory_limit_mb > 0).then(|| MemoryCache::new(memory_limit_mb));
    if MEMORY_CACHE.set(cache).is_err() {
        tracing::warn!("memory cache already initialized");
    }
}

/// Returns the memory cache, or `None` if it's disabled
pub fn memory_cache() -> Option<&'static MemoryCache> {
    MEMORY_CACHE
        .get_or_init(|| Some(MemoryCache::new(DEFAULT_MEMORY_LIMIT_MB)))
        .as_ref()
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
