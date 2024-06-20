use std::{any::Any, collections::HashMap, path::PathBuf, time::SystemTime};

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
use tokio::io::AsyncReadExt;

/// Disk based cache storage and memory cache for speeding up cache lookups

pub struct DiskCache {
    directory: PathBuf,
}

impl DiskCache {
    pub fn new() -> Self {
        DiskCache {
            directory: PathBuf::from("./tmp"),
        }
    }
}

#[derive(Serialize, Deserialize)]
pub struct DiskCacheItemMeta {
    pub created_at: SystemTime,
    pub fresh_until: SystemTime,
    pub stale_while_revalidate_sec: u32,
    pub stale_if_error_sec: u32,

    /// It's converted later on to a ResponseHeader
    pub headers: HashMap<String, String>,
}

impl From<&CacheMeta> for DiskCacheItemMeta {
    fn from(meta: &CacheMeta) -> Self {
        DiskCacheItemMeta {
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

fn convert_headers(headers: HashMap<String, String>) -> ResponseHeader {
    let mut res_headers = ResponseHeader::build_no_case(StatusCode::OK, None).unwrap();

    for (k, v) in headers {
        res_headers.insert_header(k, v).ok();
    }

    res_headers
}

#[async_trait]
impl Storage for DiskCache {
    /// Lookup the storage for the given `CacheKey`
    async fn lookup(
        &'static self,
        key: &CacheKey,
        _: &SpanHandle,
    ) -> Result<Option<(CacheMeta, HitHandler)>> {
        tracing::info!("looking up cache for {key:?}");
        // Basically we need to find a namespaced file in the cache directory
        // and return the file contents as the body

        let namespace = key.namespace();
        let primary_key = key.primary_key();
        let main_path = self.directory.join(namespace);
        let metadata_file = format!("{primary_key}.metadata");
        let cache_file = format!("{primary_key}.cache");
        let Ok(body) = tokio::fs::read(main_path.join(metadata_file)).await else {
            return Ok(None);
        };

        let Ok(meta) = serde_json::from_slice::<DiskCacheItemMeta>(&body) else {
            return Ok(None);
        };

        let Ok(file_stream) = tokio::fs::File::open(main_path.join(cache_file)).await else {
            return Ok(None);
        };
        tracing::info!("found cache for {key:?}");

        Ok(Some((
            CacheMeta::new(
                meta.fresh_until,
                meta.created_at,
                meta.stale_while_revalidate_sec,
                meta.stale_if_error_sec,
                convert_headers(meta.headers),
            ),
            Box::new(DiskCacheHitHandler::new(file_stream)),
        )))
    }

    /// Write the given [CacheMeta] to the storage. Return [MissHandler] to write the body later.
    async fn get_miss_handler(
        &'static self,
        key: &CacheKey,
        meta: &CacheMeta,
        _: &SpanHandle,
    ) -> Result<MissHandler> {
        tracing::info!("getting miss handler for {key:?}");
        let primary_key = key.primary_key();

        let main_path = PathBuf::from("./tmp").join(key.namespace());

        let metadata_file = format!("{primary_key}.metadata");

        tokio::fs::create_dir_all(&main_path).await.unwrap();

        tokio::fs::write(
            metadata_file,
            serde_json::to_vec::<DiskCacheItemMeta>(&DiskCacheItemMeta::from(meta)).unwrap(),
        )
        .await
        .ok();

        Ok(Box::new(DiskCacheMissHandler::new(
            key.to_owned(),
            DiskCacheItemMeta::from(meta),
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
        let main_path = self.directory.join(namespace);
        let metadata_file = format!("{primary_key}.metadata");

        tokio::fs::write(
            main_path.join(metadata_file),
            serde_json::to_vec::<DiskCacheItemMeta>(&DiskCacheItemMeta::from(meta)).unwrap(),
        )
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
    target: tokio::fs::File,
}

/// HIT handler for the cache
impl DiskCacheHitHandler {
    pub fn new(target: tokio::fs::File) -> Self {
        tracing::info!("creating hit handler");
        DiskCacheHitHandler { target }
    }
}

#[async_trait]
impl HandleHit for DiskCacheHitHandler {
    /// Read cached body
    ///
    /// Return `None` when no more body to read.
    async fn read_body(&mut self) -> Result<Option<bytes::Bytes>> {
        tracing::info!("reading body");

        let mut buf = Vec::with_capacity(50);

        if let Err(_) = self.target.read_to_end(&mut buf).await {
            return Ok(None);
        }

        Ok(Some(bytes::Bytes::copy_from_slice(&buf)))
    }

    /// Finish the current cache hit
    async fn finish(
        self: Box<Self>, // because self is always used as a trait object
        _: &'static (dyn Storage + Sync),
        _: &CacheKey,
        _: &SpanHandle,
    ) -> Result<()> {
        // TODO: implement flush
        // self.target.flush().await.ok();
        Ok(())
    }

    /// Whether this storage allow seeking to a certain range of body
    fn can_seek(&self) -> bool {
        false
    }

    /// Try to seek to a certain range of the body
    /// For files this could become a blocking operation
    /// `end: None` means to read to the end of the body.
    fn seek(&mut self, _: usize, _: Option<usize>) -> Result<()> {
        Ok(())
    }

    /// Helper function to cast the trait object to concrete types
    fn as_any(&self) -> &(dyn Any + Send + Sync) {
        self
    }
}

/// MISS handler for the cache
pub struct DiskCacheMissHandler {
    key: CacheKey,
    meta: DiskCacheItemMeta,
}

impl DiskCacheMissHandler {
    pub fn new(key: CacheKey, meta: DiskCacheItemMeta) -> DiskCacheMissHandler {
        DiskCacheMissHandler { key, meta }
    }
}

#[async_trait]
impl HandleMiss for DiskCacheMissHandler {
    /// Write the given body to the storage
    async fn write_body(&mut self, data: bytes::Bytes, _: bool) -> Result<()> {
        tracing::info!("writing body");
        let primary_key = self.key.primary_key();

        let main_path = PathBuf::from("./tmp").join(self.key.namespace());

        let metadata_file = format!("{primary_key}.metadata");
        let cache_file = format!("{primary_key}.cache");

        tokio::fs::create_dir_all(&main_path).await.unwrap();

        tokio::fs::write(
            metadata_file,
            serde_json::to_vec::<DiskCacheItemMeta>(&self.meta).unwrap(),
        )
        .await
        .ok();

        // TODO check performance
        tokio::fs::write(cache_file, data).await.ok();

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
