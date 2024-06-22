use std::{any::Any, path::PathBuf};

use async_trait::async_trait;

use pingora_cache::{
    key::CompactCacheKey, trace::SpanHandle, CacheKey, CacheMeta, HitHandler, MissHandler, Storage,
};

use pingora::Result;

use crate::{
    cache::disk::{
        handlers::{DiskCacheHitHandler, DiskCacheMissHandler},
        meta::DiskCacheItemMetadata,
    },
    stores,
};

/// Disk based cache storage using a `BufReader`
pub struct DiskCache {
    pub directory: PathBuf,
}

impl DiskCache {
    pub fn new() -> Self {
        DiskCache {
            directory: PathBuf::from("/tmp"),
        }
    }

    /// Retrieves the directory for the given key using the namespace as the base path
    pub fn get_directory_for(&self, key: &str) -> PathBuf {
        let Some(path) = stores::get_cache_routing_by_key(key) else {
            return self.directory.join(key);
        };

        PathBuf::from(path).join(key)
    }
}

#[async_trait]
impl Storage for DiskCache {
    /// Lookup the storage for the given `CacheKey`
    ///
    /// Whether this storage backend supports reading partially written data
    ///
    /// This is to indicate when cache should unlock readers
    fn support_streaming_partial_write(&self) -> bool {
        false
    }

    async fn lookup(
        &'static self,
        key: &CacheKey,
        _: &SpanHandle,
    ) -> Result<Option<(CacheMeta, HitHandler)>> {
        tracing::debug!("looking up cache for {key:?}");
        // Basically we need to find a namespaced file in the cache directory
        // and return the file contents as the body

        let namespace = key.namespace();
        let primary_key = key.primary_key();
        let main_path = self.get_directory_for(namespace);
        let metadata_file = format!("{primary_key}.metadata");
        let cache_file = format!("{primary_key}.cache");
        let Ok(body) = std::fs::read(main_path.join(metadata_file)) else {
            return Ok(None);
        };

        let Ok(meta) = serde_json::from_slice::<DiskCacheItemMetadata>(&body) else {
            return Ok(None);
        };

        let file_path = main_path.join(cache_file);

        let Ok(file_stream) = std::fs::OpenOptions::new().read(true).open(&file_path) else {
            return Ok(None);
        };

        // file_stream.rewind().await.ok();
        tracing::debug!("found cache for {key:?}");

        let buf_reader = std::io::BufReader::new(file_stream);

        Ok(Some((
            CacheMeta::new(
                meta.fresh_until,
                meta.created_at,
                meta.stale_while_revalidate_sec,
                meta.stale_if_error_sec,
                meta.convert_headers(),
            ),
            Box::new(DiskCacheHitHandler::new(buf_reader, file_path)),
        )))
    }

    /// Write the given [CacheMeta] to the storage. Return [MissHandler] to write the body later.
    async fn get_miss_handler(
        &'static self,
        key: &CacheKey,
        meta: &CacheMeta,
        _: &SpanHandle,
    ) -> Result<MissHandler> {
        tracing::debug!("getting miss handler for {key:?}");
        let primary_key = key.primary_key();
        let main_path = self.get_directory_for(key.namespace());
        let metadata_file = format!("{primary_key}.metadata");

        if let Err(err) = tokio::fs::create_dir_all(&main_path).await {
            tracing::error!("failed to create directory {main_path:?}: {err}");
            return Err(pingora::Error::new_str("failed to create directory"));
        }

        let Ok(serialized_metadata) =
            serde_json::to_vec::<DiskCacheItemMetadata>(&DiskCacheItemMetadata::from(meta))
        else {
            return Err(pingora::Error::new_str("failed to serialize cache meta"));
        };
        tokio::fs::write(main_path.join(metadata_file), serialized_metadata)
            .await
            .ok();

        Ok(Box::new(DiskCacheMissHandler::new(
            key.to_owned(),
            DiskCacheItemMetadata::from(meta),
            main_path,
        )))
    }

    /// Delete the cached asset for the given key
    ///
    /// [CompactCacheKey] is used here because it is how eviction managers store the keys
    async fn purge(&'static self, _: &CompactCacheKey, _: &SpanHandle) -> Result<bool> {
        Ok(true)
    }

    /// Update cache header and metadata for the already stored asset.
    async fn update_meta(
        &'static self,
        key: &CacheKey,
        meta: &CacheMeta,
        _: &SpanHandle,
    ) -> Result<bool> {
        let namespace = key.namespace();
        let primary_key = key.primary_key();
        let main_path = self.get_directory_for(namespace);
        let metadata_file = format!("{primary_key}.metadata");

        let Ok(serialized_metadata) =
            serde_json::to_vec::<DiskCacheItemMetadata>(&DiskCacheItemMetadata::from(meta))
        else {
            return Err(pingora::Error::new_str("failed to serialize cache meta"));
        };

        tokio::fs::write(main_path.join(metadata_file), serialized_metadata)
            .await
            .ok();

        Ok(true)
    }

    /// Helper function to cast the trait object to concrete types
    fn as_any(&self) -> &(dyn Any + Send + Sync + 'static) {
        self
    }
}
