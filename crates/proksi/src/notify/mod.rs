//! `notify` blocks: webhooks told about what happens to Proksi. Events are raised where they
//! happen (health checks, certificates, reloads, the store...) and sent from a service of their
//! own: raising one never waits, and costs nothing without `notify` blocks.

use std::{
    collections::BTreeMap,
    sync::{
        Arc, LazyLock, Mutex, PoisonError,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant, SystemTime},
};

use arc_swap::ArcSwap;
use async_trait::async_trait;
use pingora::{
    server::{ListenFds, ShutdownWatch},
    services::Service,
};
use tokio::sync::{Semaphore, mpsc};

use crate::config::notify::{Kind, NotifyBlock};

mod deliver;
mod hold;
mod message;

pub use deliver::masked;

/// Something that happened, and what it's about
#[derive(Debug, Clone)]
pub struct Event {
    pub kind: Kind,
    pub host: Option<String>,
    pub location: Option<String>,
    pub upstream: Option<String>,
    /// The `tcp` block of an upstream, for upstreams of `tcp` blocks
    pub tcp: Option<String>,
    /// More about it: an error, a reason
    pub detail: Option<String>,
    /// Who made a change (a token's name)
    pub author: Option<String>,
    pub version: Option<u64>,
    pub days_left: Option<i64>,
    pub at: SystemTime,
}

impl Event {
    pub fn new(kind: Kind) -> Self {
        Self {
            kind,
            host: None,
            location: None,
            upstream: None,
            tcp: None,
            detail: None,
            author: None,
            version: None,
            days_left: None,
            at: SystemTime::now(),
        }
    }

    /// What it's about, the same for both halves of a pair (an upstream going down, then up)
    fn subject(&self) -> String {
        let part = |value: &Option<String>| value.as_deref().unwrap_or("").to_string();
        let family = match self.kind {
            Kind::UpstreamDown | Kind::UpstreamUp => "upstream",
            Kind::StoreDown | Kind::StoreUp => "store",
            kind => kind.name(),
        };
        [
            family.to_string(),
            part(&self.host),
            part(&self.location),
            part(&self.tcp),
            part(&self.upstream),
        ]
        .join(" ")
    }
}

/// The blocks in use
type Blocks = BTreeMap<String, NotifyBlock>;

static BLOCKS: LazyLock<ArcSwap<Blocks>> = LazyLock::new(|| ArcSwap::from_pointee(Blocks::new()));

/// Whether there's any block: without, events aren't even built
static ENABLED: AtomicBool = AtomicBool::new(false);

/// Events waiting to be sent: when full (webhooks too slow), new ones are dropped
const QUEUED: usize = 256;

struct Queue {
    sender: mpsc::Sender<Event>,
    receiver: Mutex<Option<mpsc::Receiver<Event>>>,
}

static QUEUE: LazyLock<Queue> = LazyLock::new(|| {
    let (sender, receiver) = mpsc::channel(QUEUED);
    Queue {
        sender,
        receiver: Mutex::new(Some(receiver)),
    }
});

/// Uses `blocks` from now on (at startup, and when the configuration is reloaded)
pub fn configure(blocks: Blocks) {
    ENABLED.store(!blocks.is_empty(), Ordering::Relaxed);
    BLOCKS.store(Arc::new(blocks));
}

/// The blocks in use
pub fn blocks() -> Arc<Blocks> {
    BLOCKS.load_full()
}

/// Raises the event `event` builds: never waits, and doesn't build it without `notify` blocks
pub fn emit(event: impl FnOnce() -> Event) {
    if !ENABLED.load(Ordering::Relaxed) {
        return;
    }
    let event = event();
    let kind = event.kind;
    if QUEUE.sender.try_send(event).is_err() {
        crate::metrics::notification("", kind.name(), "dropped");
    }
}

/// The last time a block's webhook was called, and how it went
#[derive(Debug, Clone, serde::Serialize)]
pub struct Delivery {
    pub event: &'static str,
    /// Unix time, in seconds
    pub at: u64,
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

static DELIVERIES: LazyLock<Mutex<BTreeMap<String, Delivery>>> = LazyLock::new(Mutex::default);

/// The last delivery of each block
pub fn deliveries() -> BTreeMap<String, Delivery> {
    DELIVERIES
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .clone()
}

fn record(name: &str, kind: Kind, result: &Result<(), String>) {
    let delivery = Delivery {
        event: kind.name(),
        at: SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .map_or(0, |elapsed| elapsed.as_secs()),
        ok: result.is_ok(),
        error: result.clone().err(),
    };
    DELIVERIES
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .insert(name.to_string(), delivery);
    crate::metrics::notification(
        name,
        kind.name(),
        if result.is_ok() { "sent" } else { "failed" },
    );
    if let Err(err) = result {
        tracing::warn!("notify \"{name}\": couldn't send {}: {err}", kind.name());
    }
}

/// Sends a test to the block `name` now, without retrying: how it went
pub async fn test(name: &str) -> Option<Result<(), String>> {
    let block = blocks().get(name)?.clone();
    let event = Event::new(Kind::Test);
    let body = message::body(&event, block.format, &crate::stores::hostname());
    deliver::ready().await;
    let result = deliver::send(&block, &body, false).await;
    record(name, Kind::Test, &result);
    Some(result)
}

/// Certificates are checked for `certificate_expiring` from this many days left (each block
/// then says from how many it wants to know, `expiring_days`)
pub const EXPIRING_DAYS_MAX: i64 = 60;

/// Events every instance sees for itself (the same upstream going down, the same certificate
/// expiring): with a shared store, one instance sends them, for as long as `ttl`
fn claim(kind: Kind) -> Option<Duration> {
    match kind {
        Kind::UpstreamDown | Kind::UpstreamUp | Kind::StoreDown | Kind::StoreUp => Some(HELD),
        Kind::CertificateExpiring => Some(Duration::from_hours(23)),
        _ => None,
    }
}

/// How long the same thing isn't told about twice (a flapping upstream)
const HELD: Duration = Duration::from_secs(60);

/// Deliveries running at once: more are dropped (and counted)
static SENDING: Semaphore = Semaphore::const_new(16);

/// Sends `event` to every block that asks for it, in the background
fn dispatch(event: Event) {
    let blocks = blocks();
    let to: Vec<(String, NotifyBlock)> = blocks
        .iter()
        .filter(|(_, block)| {
            block.wants(event.kind)
                && block.covers(event.host.as_deref())
                && event
                    .days_left
                    .is_none_or(|days| days <= i64::from(block.expiring_days))
        })
        .map(|(name, block)| (name.clone(), block.clone()))
        .collect();
    if to.is_empty() {
        return;
    }
    let Ok(permit) = SENDING.try_acquire() else {
        for (name, _) in &to {
            crate::metrics::notification(name, event.kind.name(), "dropped");
        }
        return;
    };
    // Its own thread: the store (claiming) and webhooks may be slow, which must not stall the
    // other background services
    crate::routing::store_source::spawn_isolated(async move {
        let _permit = permit;
        if let Some(ttl) = claim(event.kind) {
            let name = format!("notify:{}:{}", event.kind.name(), event.subject());
            let store = crate::stores::global::get_store();
            // When the store can't tell (it's down), each instance sends its own
            if let Ok(false) = store
                .try_lock(&name, crate::stores::replica_id(), ttl)
                .await
            {
                return;
            }
        }
        let instance = crate::stores::hostname();
        for (name, block) in to {
            let body = message::body(&event, block.format, &instance);
            let result = deliver::send(&block, &body, true).await;
            record(&name, event.kind, &result);
        }
    });
}

/// Sends events as they're raised, for as long as Proksi runs (on shutdown, until it exits:
/// `instance_stopping` goes out while connections drain)
pub struct NotifyService;

impl NotifyService {
    /// Starts with the configuration's `notify` blocks
    pub fn new(blocks: BTreeMap<String, NotifyBlock>) -> Self {
        configure(blocks);
        Self
    }
}

#[async_trait]
impl Service for NotifyService {
    async fn start_service(
        &mut self,
        _fds: Option<ListenFds>,
        _shutdown: ShutdownWatch,
        _listeners_per_fd: usize,
    ) {
        let Some(mut receiver) = QUEUE
            .receiver
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take()
        else {
            return;
        };
        // The client is ready before the first event
        if ENABLED.load(Ordering::Relaxed) {
            deliver::ready().await;
        }
        let mut hold = hold::Hold::new(HELD);
        let mut tick = tokio::time::interval(Duration::from_secs(1));
        loop {
            tokio::select! {
                event = receiver.recv() => {
                    let Some(event) = event else { return };
                    if let Some(event) = hold.offer(event, Instant::now()) {
                        dispatch(event);
                    }
                }
                _ = tick.tick() => hold.due(Instant::now()).into_iter().for_each(dispatch),
            }
        }
    }

    fn name(&self) -> &'static str {
        "notify"
    }

    fn threads(&self) -> Option<usize> {
        Some(1)
    }
}

#[cfg(test)]
mod tests {
    use std::{
        collections::BTreeMap,
        sync::{
            Arc, Mutex,
            atomic::{AtomicBool, Ordering},
        },
        time::{Duration, Instant},
    };

    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    use super::{Event, configure, deliveries, emit, test};
    use crate::config::notify::{Format, Kind, NotifyBlock};

    /// The blocks in use are shared: these tests take turns
    static TURN: Mutex<()> = Mutex::new(());

    fn block(url: String, format: Format) -> NotifyBlock {
        NotifyBlock {
            url,
            format,
            on: vec![],
            hosts: vec![],
            headers: BTreeMap::from([("x-token".to_string(), "secret".to_string())]),
            expiring_days: 14,
        }
    }

    /// A webhook answering `204` once: the request it received
    async fn webhook() -> (String, tokio::task::JoinHandle<String>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}/hook", listener.local_addr().unwrap());
        let received = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut request = vec![0; 8192];
            let mut read = 0;
            // Until the body arrived (its end: the JSON's closing brace)
            while !request[..read].ends_with(b"}") {
                read += socket.read(&mut request[read..]).await.unwrap();
            }
            socket
                .write_all(b"HTTP/1.1 204 No Content\r\ncontent-length: 0\r\n\r\n")
                .await
                .unwrap();
            String::from_utf8_lossy(&request[..read]).to_string()
        });
        (url, received)
    }

    #[tokio::test]
    async fn test_a_webhook_is_called() {
        let _turn = TURN
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let (url, received) = webhook().await;
        configure(BTreeMap::from([(
            "local".to_string(),
            block(url, Format::Slack),
        )]));

        assert_eq!(test("local").await, Some(Ok(())));
        let request = received.await.unwrap();
        assert!(request.starts_with("POST /hook "), "{request}");
        assert!(request.contains("x-token: secret"), "{request}");
        assert!(
            request.contains(r#"{"text":"🔵 A test from Proksi: this webhook works"#),
            "{request}"
        );
        assert!(deliveries()["local"].ok);
        assert!(test("missing").await.is_none());
    }

    #[tokio::test]
    async fn test_a_failing_webhook_is_reported() {
        let _turn = TURN
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        configure(BTreeMap::from([(
            "dead".to_string(),
            block("http://127.0.0.1:1/hook".to_string(), Format::Json),
        )]));

        let Some(Err(err)) = test("dead").await else {
            panic!("a webhook nothing listens on can't be called");
        };
        // Without the URL, which can be a secret
        assert!(!err.contains("127.0.0.1:1/hook"), "{err}");
        // With why it failed, not only that it did
        assert!(err.to_lowercase().contains("connect"), "{err}");
        let last = &deliveries()["dead"];
        assert!(!last.ok);
        assert_eq!(last.event, "test");
    }

    #[test]
    fn test_raising_events_never_waits() {
        let _turn = TURN
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        configure(BTreeMap::from([(
            "dead".to_string(),
            block("http://127.0.0.1:1/hook".to_string(), Format::Json),
        )]));
        // Nothing sends them here: past the queue's size, they're dropped
        let start = Instant::now();
        for _ in 0..1000 {
            emit(|| Event::new(Kind::ConfigReloaded));
        }
        assert!(
            start.elapsed() < Duration::from_millis(100),
            "{:?}",
            start.elapsed()
        );
    }

    #[test]
    fn test_events_are_not_built_without_blocks() {
        let _turn = TURN
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        configure(BTreeMap::new());
        let built = Arc::new(AtomicBool::new(false));
        let seen = built.clone();
        emit(move || {
            seen.store(true, Ordering::Relaxed);
            Event::new(Kind::ConfigReloaded)
        });
        assert!(!built.load(Ordering::Relaxed));
    }
}
