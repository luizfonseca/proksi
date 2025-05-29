use std::{
    any::Any,
    io::Read,
    path::{Path, PathBuf},
};

use async_trait::async_trait;

// use bytes::BufMut;
use pingora_cache::{
    key::CacheHashKey,
    storage::{HandleHit, HandleMiss, MissFinishType},
    trace::SpanHandle,
    CacheKey, Storage,
};

use pingora::Result;

use tokio::{fs::OpenOptions, io::AsyncWriteExt};

use crate::cache::disk::storage::DISK_MEMORY_CACHE;

use super::meta::DiskCacheItemMetadata;

pub struct DiskCacheHitHandler {
    target: std::io::BufReader<std::fs::File>,
    path: PathBuf,

    meta: DiskCacheItemMetadata,
    finished_buffer: bytes::BytesMut,
}

/// HIT handler for the cache
impl DiskCacheHitHandler {
    pub fn new(
        target: std::io::BufReader<std::fs::File>,
        path: PathBuf,

        meta: DiskCacheItemMetadata,
    ) -> Self {
        DiskCacheHitHandler {
            target,
            path,
            meta,
            finished_buffer: bytes::BytesMut::new(),
        }
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

        self.finished_buffer
            .extend_from_slice(&buffer[..bytes_read]);
        Ok(Some(slice))
    }

    /// Finish the current cache hit
    async fn finish(
        self: Box<Self>, // because self is always used as a trait object
        _storage: &'static (dyn Storage + Sync),
        cache_key: &CacheKey,
        _: &SpanHandle,
    ) -> Result<()> {
        // Skiping if the data is already in the cache
        if let Some(existing) = DISK_MEMORY_CACHE.pin().get(&cache_key.primary()) {
            if existing.1.len() == self.finished_buffer.len() {
                tracing::debug!("skipping write, cache already contains data for {cache_key:?}");
                return Ok(());
            }
        }
        tracing::debug!("writing to memory cache: {:?}", cache_key.primary());

        DISK_MEMORY_CACHE.pin().insert(
            cache_key.primary(),
            (self.meta, self.finished_buffer.freeze()),
        );

        tracing::debug!("wrote to memory cache: {:?}", self.path);
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
    _meta: DiskCacheItemMetadata,
}

impl DiskCacheMissHandler {
    pub fn new(
        key: CacheKey,
        meta: DiskCacheItemMetadata,
        directory: PathBuf,
    ) -> DiskCacheMissHandler {
        DiskCacheMissHandler {
            key,
            _meta: meta,
            main_path: directory,
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
}

#[async_trait]
impl HandleMiss for DiskCacheMissHandler {
    /// Write the given body to the storage
    async fn write_body(&mut self, data: bytes::Bytes, end: bool) -> pingora::Result<()> {
        let primary_key = self.key.primary();
        let main_path = self.main_path.clone();
        let cache_file = format!("{primary_key}.cache");

        let Ok(_f) = Self::write_to_file(&main_path.join(&cache_file), &data).await else {
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
    ) -> Result<MissFinishType> {
        Ok(MissFinishType::Created(0))
    }
}

pub struct DiskCacheHitHandlerInMemory {
    target: bytes::buf::Reader<bytes::Bytes>,
    // path: PathBuf,
}

/// HIT handler for the cache
impl DiskCacheHitHandlerInMemory {
    pub fn new(target: bytes::buf::Reader<bytes::Bytes>) -> Self {
        DiskCacheHitHandlerInMemory { target }
    }
}

#[async_trait]
impl HandleHit for DiskCacheHitHandlerInMemory {
    /// Read cached body
    ///
    /// Return `None` when no more body to read.
    async fn read_body(&mut self) -> Result<Option<bytes::Bytes>> {
        let mut buffer = vec![0; 128_000];

        let Ok(bytes_read) = self.target.read(&mut buffer) else {
            tracing::error!("failed to read completely from MEMORY cache");
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
