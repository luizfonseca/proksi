//! Where a contract comes from (a file or a URL), loaded and kept fresh in the background:
//! requests never wait for a spec. Locations naming the same spec share one source.
//!
//! - A spec is compiled on one of a few threads of its own, a spec at a time on each, and the
//!   contract it had is kept until the new one is ready (or if it fails).
//! - Compiled contracts are kept in a cache directory, with what identifies their spec: a file's
//!   size, modification time and inode (then its SHA-256, if they changed), a URL's `ETag` and
//!   `Last-Modified`. A restart reads the cache instead of unchanged specs; a URL is asked
//!   whether it changed (`If-None-Match`, `If-Modified-Since`).

use std::{
    collections::{HashMap, VecDeque},
    io::Write,
    path::{Path, PathBuf},
    sync::{Arc, Condvar, LazyLock, Mutex, OnceLock, PoisonError, Weak},
    time::{Duration, Instant, UNIX_EPOCH},
};

use arc_swap::ArcSwapOption;
use serde::{Deserialize, Serialize};

use super::{
    contract::{Contract, Plan},
    spec,
};

/// Threads compiling specs: a few, so that hundreds of large specs don't take the machine
const WORKERS: usize = 2;

/// The largest spec downloaded from a URL
const MAX_DOWNLOAD: u64 = 1024 * 1024 * 1024;

const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(120);

/// The cache's format: other formats are compiled again
const CACHE_FORMAT: u32 = 1;

/// Where a spec is
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(super) enum Origin {
    File(PathBuf),
    Url(String),
}

/// A spec, and its contract once loaded
pub(super) struct Source {
    origin: Origin,
    refresh: Duration,
    contract: ArcSwapOption<Contract>,
    state: Mutex<State>,
}

#[derive(Default)]
struct State {
    /// What identifies the spec the contract is from
    loaded: Option<Fingerprint>,
    /// When to look at the spec again
    next: Option<Instant>,
    queued: bool,
}

/// What identifies a spec's content
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
struct Fingerprint {
    size: u64,
    /// Nanoseconds since the epoch (files)
    modified: Option<u128>,
    inode: Option<u64>,
    sha256: String,
    etag: Option<String>,
    last_modified: Option<String>,
}

#[derive(Serialize, Deserialize)]
struct Cached {
    format: u32,
    origin: String,
    fingerprint: Fingerprint,
    plan: Plan,
}

static CACHE_DIR: OnceLock<PathBuf> = OnceLock::new();

/// Sources, by origin: the locations naming one hold it, and it goes with the last of them
static SOURCES: LazyLock<Mutex<HashMap<Origin, Weak<Source>>>> = LazyLock::new(Mutex::default);

static QUEUE: LazyLock<(Mutex<VecDeque<Arc<Source>>>, Condvar)> =
    LazyLock::new(|| (Mutex::default(), Condvar::new()));

static STARTED: OnceLock<()> = OnceLock::new();

/// Whether the cache directory was set (or won't be): specs are only loaded after, so that
/// those cached are read from the cache
static CONFIGURED: LazyLock<(Mutex<bool>, Condvar)> =
    LazyLock::new(|| (Mutex::new(false), Condvar::new()));

/// How long specs wait for the cache directory, set once the configuration is read
const CONFIGURE_TIMEOUT: Duration = Duration::from_secs(5);

/// Keeps compiled contracts in `dir` (created if needed), read again after a restart. Specs
/// are loaded once it's set (or after a few seconds without it).
pub fn set_cache_dir(dir: PathBuf) {
    let _ = CACHE_DIR.set(dir);
    let (configured, ready) = &*CONFIGURED;
    *lock(configured) = true;
    ready.notify_all();
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

impl Origin {
    fn describe(&self) -> String {
        match self {
            Origin::File(path) => path.to_string_lossy().into_owned(),
            Origin::Url(url) => url.clone(),
        }
    }
}

impl Source {
    /// The source of `origin`, shared with the other locations naming it, loading in the
    /// background if it's new
    pub(super) fn of(origin: Origin, refresh: Duration) -> Arc<Source> {
        let mut sources = lock(&SOURCES);
        sources.retain(|_, source| source.strong_count() > 0);
        if let Some(source) = sources.get(&origin).and_then(Weak::upgrade) {
            return source;
        }
        let source = Arc::new(Source {
            origin: origin.clone(),
            refresh,
            contract: ArcSwapOption::empty(),
            state: Mutex::default(),
        });
        sources.insert(origin, Arc::downgrade(&source));
        drop(sources);
        start();
        source.enqueue();
        source
    }

    /// The contract, once loaded
    pub(super) fn contract(&self) -> arc_swap::Guard<Option<Arc<Contract>>> {
        self.contract.load()
    }

    fn enqueue(self: &Arc<Self>) {
        let mut state = lock(&self.state);
        if state.queued {
            return;
        }
        state.queued = true;
        drop(state);
        let (queue, ready) = &*QUEUE;
        lock(queue).push_back(Arc::clone(self));
        ready.notify_one();
    }

    /// What the cache has for this spec, used until the spec is looked at (and after, if it
    /// didn't change)
    fn load_cache(&self) {
        let Some(cached) = read_cache(&self.origin) else {
            return;
        };
        self.contract
            .store(Some(Arc::new(Contract::new(&cached.plan))));
        lock(&self.state).loaded = Some(cached.fingerprint);
    }

    /// Looks at the spec, and compiles it if it changed
    fn load(&self) {
        if self.contract.load().is_none() {
            self.load_cache();
        }
        let started = Instant::now();
        let result = match &self.origin {
            Origin::File(path) => self.load_file(path),
            Origin::Url(url) => self.load_url(url),
        };
        let mut state = lock(&self.state);
        state.queued = false;
        state.next = Some(Instant::now() + self.refresh);
        drop(state);
        match result {
            Ok(Some(paths)) => tracing::info!(
                spec = self.origin.describe(),
                paths,
                "openapi: compiled in {:?}",
                started.elapsed()
            ),
            Ok(None) => {}
            Err(err) => tracing::warn!(
                spec = self.origin.describe(),
                "openapi: can't load the spec, the last contract is kept: {err}"
            ),
        }
    }

    /// `Some(paths)` when the spec was compiled, `None` when it hadn't changed
    fn load_file(&self, path: &Path) -> Result<Option<usize>, String> {
        let metadata = std::fs::metadata(path).map_err(|err| err.to_string())?;
        let mut fingerprint = file_fingerprint(&metadata);
        let loaded = lock(&self.state).loaded.clone();
        let same_file = |known: &Fingerprint| {
            known.size == fingerprint.size
                && known.modified == fingerprint.modified
                && known.inode == fingerprint.inode
        };
        if loaded.as_ref().is_some_and(same_file) {
            return Ok(None);
        }

        // Read from a copy of its own: a spec file cut short while it's mapped (rewritten in
        // place) would stop proksi (`SIGBUS`); the copy can't change under it
        let mut copy = tempfile_in_cache()?;
        let mut file = std::fs::File::open(path).map_err(|err| err.to_string())?;
        std::io::copy(&mut file, &mut copy).map_err(|err| err.to_string())?;
        // SAFETY: a file of our own, not changed while it's read
        let bytes = unsafe { memmap2::Mmap::map(&copy) }.map_err(|err| err.to_string())?;
        fingerprint.sha256 = sha256(&bytes);
        // Touched, copied: the same content
        if loaded
            .as_ref()
            .is_some_and(|known| known.sha256 == fingerprint.sha256)
        {
            self.keep(fingerprint);
            return Ok(None);
        }
        let plan = spec::compile(&bytes)?;
        drop(bytes);
        Ok(Some(self.install(plan, fingerprint)))
    }

    fn load_url(&self, url: &str) -> Result<Option<usize>, String> {
        let loaded = lock(&self.state).loaded.clone();
        let Some((file, mut fingerprint)) = download(url, loaded.as_ref())? else {
            return Ok(None);
        };
        // SAFETY: a file of our own, not changed while it's read
        let bytes = unsafe { memmap2::Mmap::map(&file) }.map_err(|err| err.to_string())?;
        fingerprint.sha256 = sha256(&bytes);
        fingerprint.size = bytes.len() as u64;
        if loaded
            .as_ref()
            .is_some_and(|known| known.sha256 == fingerprint.sha256)
        {
            self.keep(fingerprint);
            return Ok(None);
        }
        let plan = spec::compile(&bytes)?;
        Ok(Some(self.install(plan, fingerprint)))
    }

    /// The same spec as loaded, now identified by `fingerprint`
    fn keep(&self, fingerprint: Fingerprint) {
        lock(&self.state).loaded = Some(fingerprint.clone());
        if let Some(mut cached) = read_cache(&self.origin) {
            cached.fingerprint = fingerprint;
            write_cache(&self.origin, &cached);
        }
    }

    /// A new contract, in use and cached: its number of paths
    fn install(&self, plan: Plan, fingerprint: Fingerprint) -> usize {
        let paths = plan.paths.len();
        self.contract.store(Some(Arc::new(Contract::new(&plan))));
        lock(&self.state).loaded = Some(fingerprint.clone());
        let cached = Cached {
            format: CACHE_FORMAT,
            origin: self.origin.describe(),
            fingerprint,
            plan,
        };
        write_cache(&self.origin, &cached);
        paths
    }
}

fn file_fingerprint(metadata: &std::fs::Metadata) -> Fingerprint {
    #[cfg(unix)]
    let inode = Some(std::os::unix::fs::MetadataExt::ino(metadata));
    #[cfg(not(unix))]
    let inode = None;
    Fingerprint {
        size: metadata.len(),
        modified: metadata
            .modified()
            .ok()
            .and_then(|modified| modified.duration_since(UNIX_EPOCH).ok())
            .map(|since| since.as_nanos()),
        inode,
        ..Fingerprint::default()
    }
}

fn sha256(bytes: &[u8]) -> String {
    use std::fmt::Write;
    let digest = ring::digest::digest(&ring::digest::SHA256, bytes);
    digest
        .as_ref()
        .iter()
        .fold(String::with_capacity(64), |mut text, byte| {
            let _ = write!(text, "{byte:02x}");
            text
        })
}

/// The spec at `url`, written to a temporary file, unless it didn't change since `loaded`
fn download(
    url: &str,
    loaded: Option<&Fingerprint>,
) -> Result<Option<(std::fs::File, Fingerprint)>, String> {
    thread_local! {
        static RUNTIME: Option<tokio::runtime::Runtime> = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .ok();
    }
    RUNTIME.with(|runtime| {
        let runtime = runtime.as_ref().ok_or("no runtime to download with")?;
        runtime.block_on(fetch(url, loaded))
    })
}

async fn fetch(
    url: &str,
    loaded: Option<&Fingerprint>,
) -> Result<Option<(std::fs::File, Fingerprint)>, String> {
    static CLIENT: LazyLock<reqwest::Client> = LazyLock::new(reqwest::Client::new);
    let mut request = CLIENT.get(url).timeout(DOWNLOAD_TIMEOUT);
    if let Some(etag) = loaded.and_then(|loaded| loaded.etag.as_deref()) {
        request = request.header(http::header::IF_NONE_MATCH, etag);
    }
    if let Some(date) = loaded.and_then(|loaded| loaded.last_modified.as_deref()) {
        request = request.header(http::header::IF_MODIFIED_SINCE, date);
    }
    let mut response = request.send().await.map_err(|err| err.to_string())?;
    if response.status() == http::StatusCode::NOT_MODIFIED {
        return Ok(None);
    }
    if !response.status().is_success() {
        return Err(format!("{url} answered {}", response.status()));
    }
    let header = |name| {
        response
            .headers()
            .get(name)
            .and_then(|value: &http::HeaderValue| value.to_str().ok())
            .map(ToString::to_string)
    };
    let fingerprint = Fingerprint {
        etag: header(http::header::ETAG),
        last_modified: header(http::header::LAST_MODIFIED),
        ..Fingerprint::default()
    };
    // Written to a file as it arrives: a spec is never held in memory whole
    let mut file = tempfile_in_cache()?;
    let mut size = 0u64;
    while let Some(chunk) = response.chunk().await.map_err(|err| err.to_string())? {
        size += chunk.len() as u64;
        if size > MAX_DOWNLOAD {
            return Err(format!("{url} is larger than {MAX_DOWNLOAD} bytes"));
        }
        file.write_all(&chunk).map_err(|err| err.to_string())?;
    }
    file.flush().map_err(|err| err.to_string())?;
    Ok(Some((file, fingerprint)))
}

/// A file that's removed once closed, in the cache directory (or the system's temporary one)
fn tempfile_in_cache() -> Result<std::fs::File, String> {
    let dir = CACHE_DIR.get().cloned().unwrap_or_else(std::env::temp_dir);
    let _ = std::fs::create_dir_all(&dir);
    tempfile::tempfile_in(dir).map_err(|err| err.to_string())
}

fn cache_path(origin: &Origin) -> Option<PathBuf> {
    let key = match origin {
        Origin::File(path) => format!("file:{}", path.to_string_lossy()),
        Origin::Url(url) => format!("url:{url}"),
    };
    let name = sha256(key.as_bytes());
    Some(CACHE_DIR.get()?.join(format!("{}.json", name.get(..32)?)))
}

fn read_cache(origin: &Origin) -> Option<Cached> {
    let bytes = std::fs::read(cache_path(origin)?).ok()?;
    let cached: Cached = serde_json::from_slice(&bytes).ok()?;
    (cached.format == CACHE_FORMAT && cached.origin == origin.describe()).then_some(cached)
}

/// Writes the cache entry whole, or not at all (written aside, then renamed)
fn write_cache(origin: &Origin, cached: &Cached) {
    let Some(path) = cache_path(origin) else {
        return;
    };
    let written = (|| -> std::io::Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let aside = path.with_extension("json.new");
        std::fs::write(
            &aside,
            serde_json::to_vec(cached).map_err(std::io::Error::other)?,
        )?;
        std::fs::rename(aside, &path)
    })();
    if let Err(err) = written {
        tracing::warn!(
            spec = origin.describe(),
            "openapi: can't write the cache: {err}"
        );
    }
}

/// The threads compiling specs, and the one looking at them again: started once
fn start() {
    STARTED.get_or_init(|| {
        for worker in 0..WORKERS {
            let _ = std::thread::Builder::new()
                .name(format!("openapi-{worker}"))
                .spawn(work);
        }
        let _ = std::thread::Builder::new()
            .name("openapi-refresh".to_string())
            .spawn(refresh);
    });
}

fn work() {
    let (configured, set) = &*CONFIGURED;
    let _ = set
        .wait_timeout_while(lock(configured), CONFIGURE_TIMEOUT, |configured| {
            !*configured
        })
        .map_err(PoisonError::into_inner);
    let (queue, ready) = &*QUEUE;
    loop {
        let mut waiting = lock(queue);
        while waiting.is_empty() {
            waiting = ready.wait(waiting).unwrap_or_else(PoisonError::into_inner);
        }
        let Some(source) = waiting.pop_front() else {
            continue;
        };
        drop(waiting);
        // Only the queue holds it: the location naming it is gone (or was only built to check a
        // configuration)
        if Arc::strong_count(&source) == 1 {
            lock(&source.state).queued = false;
            continue;
        }
        source.load();
    }
}

/// Every second, the sources due for a look are queued
fn refresh() {
    loop {
        std::thread::sleep(Duration::from_secs(1));
        queue_due(Instant::now());
    }
}

fn queue_due(now: Instant) {
    let sources: Vec<Arc<Source>> = lock(&SOURCES).values().filter_map(Weak::upgrade).collect();
    for source in sources {
        let due = lock(&source.state).next.is_some_and(|next| next <= now);
        if due {
            source.enqueue();
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{
        path::Path,
        sync::Arc,
        time::{Duration, Instant},
    };

    use http::{HeaderMap, Method};

    use super::{cache_path, set_cache_dir, Origin, Source, CACHE_DIR};

    fn spec(paths: &[&str]) -> String {
        let paths: Vec<String> = paths
            .iter()
            .map(|path| format!(r#""{path}": {{ "get": {{ "responses": {{}} }} }}"#))
            .collect();
        format!(
            r#"{{ "openapi": "3.0.3", "paths": {{ {} }} }}"#,
            paths.join(", ")
        )
    }

    fn allows(source: &Source, path: &str) -> bool {
        let contract = source.contract();
        contract.as_ref().is_some_and(|contract| {
            contract
                .check(&Method::GET, path, None, &HeaderMap::new())
                .is_ok()
        })
    }

    fn wait_for(what: impl Fn() -> bool) -> bool {
        let started = Instant::now();
        while started.elapsed() < Duration::from_secs(10) {
            if what() {
                return true;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        false
    }

    #[test]
    fn test_specs_are_loaded_cached_and_followed() {
        let dir = tempfile::tempdir().unwrap();
        set_cache_dir(dir.path().join("cache"));
        let cache = CACHE_DIR.get().unwrap().clone();
        let file = dir.path().join("api.json");
        std::fs::write(&file, spec(&["/a"])).unwrap();
        let origin = Origin::File(file.clone());

        // Loaded in the background, and cached
        let source = Source::of(origin.clone(), Duration::from_millis(100));
        assert!(wait_for(|| allows(&source, "/a")));
        let cached = cache_path(&origin).unwrap();
        assert!(cached.starts_with(&cache) && Path::new(&cached).exists());

        // A change is followed
        std::fs::write(&file, spec(&["/a", "/b"])).unwrap();
        assert!(wait_for(|| allows(&source, "/b")));

        // A restart reads the cache, without compiling the spec again
        drop(source);
        let compiled = std::fs::metadata(&cached).unwrap().modified().unwrap();
        let again = Source::of(origin, Duration::from_secs(3600));
        assert!(wait_for(|| allows(&again, "/b")));
        assert_eq!(
            std::fs::metadata(&cached).unwrap().modified().unwrap(),
            compiled
        );
        assert!(Arc::strong_count(&again) >= 1);
    }

    #[test]
    fn test_a_spec_that_fails_keeps_the_last_contract() {
        let dir = tempfile::tempdir().unwrap();
        set_cache_dir(dir.path().join("cache"));
        let file = dir.path().join("broken.json");
        std::fs::write(&file, spec(&["/kept"])).unwrap();
        let source = Source::of(Origin::File(file.clone()), Duration::from_millis(100));
        assert!(wait_for(|| allows(&source, "/kept")));

        std::fs::write(&file, "{ not json").unwrap();
        std::thread::sleep(Duration::from_millis(2500));
        assert!(allows(&source, "/kept"));
    }
}
