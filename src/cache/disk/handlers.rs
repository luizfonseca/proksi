use std::{
    any::Any,
    collections::HashMap,
    io::{BufReader, Read},
    path::{Path, PathBuf},
    sync::Arc,
};

use async_trait::async_trait;

use pingora_cache::{
    storage::{HandleHit, HandleMiss},
    trace::SpanHandle,
    CacheKey, Storage,
};

use pingora::Result;

use tokio::{fs::OpenOptions, io::AsyncWriteExt, sync::RwLock};

use super::meta::DiskCacheItemMetadata;

type MemoryCache = Arc<RwLock<HashMap<String, (DiskCacheItemMetadata, bytes::Bytes)>>>;

pub struct DiskCacheHitHandler {
    target: BufReader<std::fs::File>,
    path: PathBuf,
    cache: MemoryCache,
    meta: DiskCacheItemMetadata,
    finished_buffer: bytes::BytesMut,
}

/// HIT handler for the cache
impl DiskCacheHitHandler {
    pub fn new(
        target: BufReader<std::fs::File>,
        path: PathBuf,
        cache: MemoryCache,
        meta: DiskCacheItemMetadata,
    ) -> Self {
        DiskCacheHitHandler {
            target,
            path,
            cache,
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
        let cached_data_key = format!("{}-{}", cache_key.namespace(), cache_key.primary_key());

        // Skiping if the data is already in the cache
        if self.cache.read().await.contains_key(&cached_data_key) {
            tracing::debug!("skipping write, cach already contains data for {cache_key:?}");
            return Ok(());
        }

        self.cache
            .write()
            .await
            .insert(cached_data_key, (self.meta, self.finished_buffer.freeze()));

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
    async fn write_body(&mut self, data: bytes::Bytes, end: bool) -> Result<()> {
        let primary_key = self.key.primary_key();
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
    ) -> pingora::Result<usize> {
        Ok(0)
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
