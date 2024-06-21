use std::{
    any::Any,
    collections::HashMap,
    io::Read,
    path::{Path, PathBuf},
    time::SystemTime,
};

use async_trait::async_trait;

use http::StatusCode;
use pingora_cache::{
    key::CompactCacheKey,
    storage::{HandleHit, HandleMiss},
    trace::SpanHandle,
    CacheKey, CacheMeta, HitHandler, MissHandler, Storage,
};

use pingora::{http::ResponseHeader, Result};
use serde::{Deserialize, Serialize};
use tokio::{fs::OpenOptions, io::AsyncWriteExt};

use crate::stores;

/// Disk based cache storage and memory cache for speeding up cache lookups

pub struct DiskCache {
    pub directory: PathBuf,
}

impl DiskCache {
    pub fn new() -> Self {
        DiskCache {
            directory: PathBuf::from("/tmp"),
        }
    }

    /// Retrieves the directory for the given key
    pub fn get_directory_for(&self, key: &str) -> PathBuf {
        let Some(path) = stores::get_cache_routing_by_key(key) else {
            return self.directory.join(key);
        };

        PathBuf::from(path).join(key)
    }
}

#[derive(Serialize, Deserialize)]
pub struct DiskCacheItemMeta {
    pub status: u16,
    pub created_at: SystemTime,
    pub fresh_until: SystemTime,
    pub stale_while_revalidate_sec: u32,
    pub stale_if_error_sec: u32,

    /// It's converted later on to a `ResponseHeader`
    pub headers: HashMap<String, String>,
}

impl From<&CacheMeta> for DiskCacheItemMeta {
    fn from(meta: &CacheMeta) -> Self {
        DiskCacheItemMeta {
            status: meta.response_header().status.as_u16(),
            created_at: meta.created(),
            fresh_until: meta.fresh_until(),
            stale_while_revalidate_sec: meta.stale_while_revalidate_sec(),
            stale_if_error_sec: meta.stale_if_error_sec(),
            headers: meta
                .headers()
                .into_iter()
                .map(|(k, v)| (k.to_string(), v.to_str().unwrap().to_string()))
                .collect(),
        }
    }
}

fn convert_headers(headers: HashMap<String, String>, status: u16) -> ResponseHeader {
    let status_code = StatusCode::from_u16(status).unwrap_or(StatusCode::OK);
    let mut res_headers = ResponseHeader::build(status_code, None).unwrap();

    for (k, v) in headers {
        res_headers.insert_header(k, v).ok();
    }

    res_headers
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

        let Ok(meta) = serde_json::from_slice::<DiskCacheItemMeta>(&body) else {
            return Ok(None);
        };

        let file_path = main_path.join(cache_file);

        let Ok(file_stream) = std::fs::OpenOptions::new().read(true).open(&file_path) else {
            return Ok(None);
        };

        // file_stream.rewind().await.ok();
        tracing::debug!("found cache for {key:?}");

        Ok(Some((
            CacheMeta::new(
                meta.fresh_until,
                meta.created_at,
                meta.stale_while_revalidate_sec,
                meta.stale_if_error_sec,
                convert_headers(meta.headers, meta.status),
            ),
            Box::new(DiskCacheHitHandler::new(file_stream, file_path)),
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
            serde_json::to_vec::<DiskCacheItemMeta>(&DiskCacheItemMeta::from(meta))
        else {
            return Err(pingora::Error::new_str("failed to serialize cache meta"));
        };
        tokio::fs::write(main_path.join(metadata_file), serialized_metadata)
            .await
            .ok();

        Ok(Box::new(DiskCacheMissHandler::new(
            key.to_owned(),
            DiskCacheItemMeta::from(meta),
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
            serde_json::to_vec::<DiskCacheItemMeta>(&DiskCacheItemMeta::from(meta))
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

pub struct DiskCacheHitHandler {
    target: std::fs::File,
    path: PathBuf,
}

/// HIT handler for the cache
impl DiskCacheHitHandler {
    pub fn new(target: std::fs::File, path: PathBuf) -> Self {
        DiskCacheHitHandler { target, path }
    }
}

#[async_trait]
impl HandleHit for DiskCacheHitHandler {
    /// Read cached body
    ///
    /// Return `None` when no more body to read.
    async fn read_body(&mut self) -> Result<Option<bytes::Bytes>> {
        let mut buffer = [0; 32_000];

        let Ok(bytes_read) = self.target.by_ref().read(&mut buffer) else {
            tracing::error!("failed to read completely from cache: {:?}", self.path);
            return Ok(None);
        };

        tracing::debug!("read from cache: {bytes_read}");
        if bytes_read == 0 {
            return Ok(None);
        }

        Ok(Some(bytes::Bytes::copy_from_slice(&buffer)))
    }

    /// Finish the current cache hit
    async fn finish(
        self: Box<Self>, // because self is always used as a trait object
        _storage: &'static (dyn Storage + Sync),
        _cache_key: &CacheKey,
        _: &SpanHandle,
    ) -> Result<()> {
        Ok(())
    }

    /// Whether this storage allow seeking to a certain range of body
    fn can_seek(&self) -> bool {
        false
    }

    /// Try to seek to a certain range of the body
    /// For files this could become a blocking operation
    /// `end: None` means to read to the end of the body.
    fn seek(&mut self, _start: usize, _end: Option<usize>) -> Result<()> {
        Ok(())
    }

    /// Helper function to cast the trait object to concrete types
    fn as_any(&self) -> &(dyn Any + Send + Sync) {
        self
    }
}

/// MISS handler for the cache
pub struct DiskCacheMissHandler {
    main_path: PathBuf,
    key: CacheKey,
    _meta: DiskCacheItemMeta,
}

impl DiskCacheMissHandler {
    pub fn new(key: CacheKey, meta: DiskCacheItemMeta, directory: PathBuf) -> DiskCacheMissHandler {
        DiskCacheMissHandler {
            key,
            _meta: meta,
            main_path: directory,
        }
    }
}

/// Writes a file to disk and append data on every write
async fn write_to_file<P: AsRef<Path>>(
    path: P,
    content: &[u8],
) -> std::io::Result<tokio::fs::File> {
    let mut file = OpenOptions::new()
        .create(true) // Create the file if it doesn't exist
        .append(true)
        .open(path)
        .await?;

    // Writing the content to the file
    file.write_all(content).await?;
    Ok(file)
}

#[async_trait]
impl HandleMiss for DiskCacheMissHandler {
    /// Write the given body to the storage
    async fn write_body(&mut self, data: bytes::Bytes, end: bool) -> Result<()> {
        let primary_key = self.key.primary_key();
        let main_path = self.main_path.clone();
        let cache_file = format!("{primary_key}.cache");

        let Ok(_f) = write_to_file(&main_path.join(&cache_file), &data).await else {
            tracing::error!(
                "failed to write to cache file: {:?}",
                main_path.join(cache_file)
            );
            return Err(pingora::Error::new_str("failed to write to cache file"));
        };

        if end {
            // file.flush().await.ok();
            return Ok(());
        }

        Ok(())
    }

    /// Finish the cache admission
    ///
    /// When `self` is dropped without calling this function, the storage should consider this write
    /// failed.
    async fn finish(
        self: Box<Self>, // because self is always used as a trait object
    ) -> Result<usize> {
        Ok(0)
    }
}
