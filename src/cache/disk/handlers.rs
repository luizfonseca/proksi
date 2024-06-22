use std::{
    any::Any,
    collections::BTreeMap,
    io::{BufReader, Read},
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

use super::meta::DiskCacheItemMeta;

pub struct DiskCacheHitHandler {
    target: BufReader<std::fs::File>,
    path: PathBuf,
}

/// HIT handler for the cache
impl DiskCacheHitHandler {
    pub fn new(target: BufReader<std::fs::File>, path: PathBuf) -> Self {
        DiskCacheHitHandler { target, path }
    }
}

#[async_trait]
impl HandleHit for DiskCacheHitHandler {
    /// Read cached body
    ///
    /// Return `None` when no more body to read.
    async fn read_body(&mut self) -> Result<Option<bytes::Bytes>> {
        let mut buffer = vec![0; 32_000];

        let Ok(bytes_read) = self.target.read(&mut buffer) else {
            tracing::error!("failed to read completely from cache: {:?}", self.path);
            return Ok(None);
        };

        tracing::debug!("read from cache: {bytes_read}");
        if bytes_read == 0 {
            return Ok(None);
        }

        let slice = bytes::Bytes::copy_from_slice(&buffer[..bytes_read]);
        Ok(Some(slice))
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
