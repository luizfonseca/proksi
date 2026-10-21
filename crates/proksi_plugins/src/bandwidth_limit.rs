//! `bandwidth_limit { rate = "1MiB", burst = "10MiB" }`: paces the responses sent to each client

use std::{
    net::IpAddr,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Mutex, PoisonError,
    },
    time::{Duration, Instant},
};

use bytes::Bytes;
use pingora::proxy::Session;

use crate::{body_limit::parse_size, RequestCtx, RoutePlugin};

use super::Middleware;

/// Shorter waits are carried over to the next part of the body, rather than slept
const SHORTEST_WAIT: Duration = Duration::from_millis(1);

/// Buckets of clients that stopped downloading are dropped once this many new ones were added
const SWEEP_EVERY: usize = 1024;

const NANOS_PER_SEC: u128 = 1_000_000_000;

/// A token bucket for each client (by IP address), shared by all its responses: `burst` bytes
/// can be sent at once, then `rate` bytes per second
pub(super) struct BandwidthLimit {
    /// Bytes per second
    rate: u64,
    /// Bytes
    burst: i64,
    buckets: papaya::HashMap<IpAddr, Mutex<Bucket>>,
    added: AtomicUsize,
}

struct Bucket {
    /// Bytes that can be sent now; below 0, owed
    tokens: i64,
    /// When `tokens` was counted
    at: Instant,
}

impl BandwidthLimit {
    pub(super) fn new(plugin: &RoutePlugin) -> Result<Self, String> {
        let settings: super::settings::BandwidthLimit =
            super::settings::read("bandwidth_limit", plugin)?;
        let rate = parse_size(&settings.rate)
            .filter(|rate| *rate > 0)
            .ok_or_else(|| {
                format!(
                    "bandwidth_limit: rate \"{}\" is not a size per second such as \"512KiB\" or \"1MiB\"",
                    settings.rate
                )
            })?;
        let burst = match &settings.burst {
            None => rate,
            Some(burst) => parse_size(burst).filter(|burst| *burst > 0).ok_or_else(|| {
                format!(
                    "bandwidth_limit: burst \"{burst}\" is not a size such as \"1MiB\" or \"10MiB\""
                )
            })?,
        };
        Ok(Self {
            rate,
            burst: i64::try_from(burst).unwrap_or(i64::MAX),
            buckets: papaya::HashMap::new(),
            added: AtomicUsize::new(0),
        })
    }

    /// Counts `bytes` more sent to `client`, returning how long to wait before sending them
    fn take(&self, client: IpAddr, bytes: u64, now: Instant) -> Option<Duration> {
        let buckets = self.buckets.pin();
        if buckets.get(&client).is_none() {
            self.sweep(&buckets, now);
        }
        let bucket = buckets.get_or_insert_with(client, || {
            Mutex::new(Bucket {
                tokens: self.burst,
                at: now,
            })
        });
        let mut bucket = bucket.lock().unwrap_or_else(PoisonError::into_inner);
        let refilled = bucket
            .tokens
            .saturating_add(self.refill(now.saturating_duration_since(bucket.at)))
            .min(self.burst);
        bucket.tokens = refilled.saturating_sub(i64::try_from(bytes).unwrap_or(i64::MAX));
        bucket.at = now;

        let owed = bucket.tokens.checked_neg().and_then(|owed| u64::try_from(owed).ok())?;
        let wait = u128::from(owed) * NANOS_PER_SEC / u128::from(self.rate);
        let wait = Duration::from_nanos(u64::try_from(wait).unwrap_or(u64::MAX));
        (wait >= SHORTEST_WAIT).then_some(wait)
    }

    /// The bytes `rate` allows in `elapsed`
    fn refill(&self, elapsed: Duration) -> i64 {
        let bytes = elapsed.as_nanos() * u128::from(self.rate) / NANOS_PER_SEC;
        i64::try_from(bytes).unwrap_or(i64::MAX)
    }

    /// Every [`SWEEP_EVERY`] new clients, drops the buckets that refilled: their clients
    /// would start with a full bucket anyway
    fn sweep<S: std::hash::BuildHasher, G: papaya::Guard>(
        &self,
        buckets: &papaya::HashMapRef<'_, IpAddr, Mutex<Bucket>, S, G>,
        now: Instant,
    ) {
        if self.added.fetch_add(1, Ordering::Relaxed) % SWEEP_EVERY != SWEEP_EVERY - 1 {
            return;
        }
        buckets.retain(|_, bucket| {
            let bucket = bucket.lock().unwrap_or_else(PoisonError::into_inner);
            let refill = self.refill(now.saturating_duration_since(bucket.at));
            bucket.tokens.saturating_add(refill) < self.burst
        });
    }
}

impl Middleware for BandwidthLimit {
    fn response_body(
        &self,
        session: &Session,
        body: Option<&Bytes>,
        ctx: &mut dyn RequestCtx,
    ) -> Option<Duration> {
        let bytes = body.map_or(0, Bytes::len);
        if bytes == 0 {
            return None;
        }
        let client = ctx.client(session)?;
        self.take(client, u64::try_from(bytes).unwrap_or(u64::MAX), Instant::now())
    }
}

#[cfg(test)]
mod tests {
    use std::{
        net::IpAddr,
        time::{Duration, Instant},
    };

    use crate::RoutePlugin;

    use super::{BandwidthLimit, SWEEP_EVERY};

    const KIB: u64 = 1024;

    fn limit(config: serde_json::Value) -> Result<BandwidthLimit, String> {
        BandwidthLimit::new(&RoutePlugin {
            name: "bandwidth_limit".into(),
            config: serde_json::from_value(config).unwrap(),
        })
    }

    fn client(last: u8) -> IpAddr {
        IpAddr::from([203, 0, 113, last])
    }

    #[test]
    fn test_settings() {
        let block = limit(serde_json::json!({ "rate": "1MiB", "burst": "10MiB" })).unwrap();
        assert_eq!((block.rate, block.burst), (1024 * KIB, 10 * 1024 * 1024));
        // One second of `rate` unless set
        let block = limit(serde_json::json!({ "rate": "512KiB" })).unwrap();
        assert_eq!(block.burst, 512 * 1024);

        for (config, err) in [
            (
                serde_json::json!({ "rate": "fast" }),
                "bandwidth_limit: rate \"fast\" is not a size per second such as \"512KiB\" or \"1MiB\"",
            ),
            (
                serde_json::json!({ "rate": "0" }),
                "bandwidth_limit: rate \"0\" is not a size per second such as \"512KiB\" or \"1MiB\"",
            ),
            (
                serde_json::json!({ "rate": "1MiB", "burst": "0" }),
                "bandwidth_limit: burst \"0\" is not a size such as \"1MiB\" or \"10MiB\"",
            ),
            (serde_json::json!({}), "bandwidth_limit: `rate` is required"),
        ] {
            assert_eq!(limit(config).err().as_deref(), Some(err));
        }
    }

    #[test]
    fn test_the_burst_is_sent_at_once_then_the_rate() {
        let block = limit(serde_json::json!({ "rate": "100KiB", "burst": "200KiB" })).unwrap();
        let start = Instant::now();

        assert_eq!(block.take(client(1), 200 * KIB, start), None);
        // 50 KiB more is owed: half a second at 100 KiB/s
        assert_eq!(
            block.take(client(1), 50 * KIB, start),
            Some(Duration::from_millis(500))
        );
        // Half a second later, it's paid
        let later = start + Duration::from_millis(500);
        assert_eq!(block.take(client(1), 100 * KIB, later), Some(Duration::from_secs(1)));
    }

    #[test]
    fn test_clients_have_their_own_bucket() {
        let block = limit(serde_json::json!({ "rate": "100KiB" })).unwrap();
        let now = Instant::now();
        assert_eq!(block.take(client(1), 100 * KIB, now), None);
        assert_eq!(block.take(client(2), 100 * KIB, now), None);
        assert!(block.take(client(1), KIB, now).is_some());
    }

    #[test]
    fn test_an_idle_client_gets_its_burst_back_and_no_more() {
        let block = limit(serde_json::json!({ "rate": "100KiB" })).unwrap();
        let start = Instant::now();
        block.take(client(1), 100 * KIB, start);

        let much_later = start + Duration::from_secs(3600);
        assert_eq!(block.take(client(1), 100 * KIB, much_later), None);
        assert!(block.take(client(1), KIB, much_later).is_some());
    }

    #[test]
    fn test_waits_under_a_millisecond_are_carried_over() {
        let block = limit(serde_json::json!({ "rate": "1MiB" })).unwrap();
        let now = Instant::now();
        block.take(client(1), 1024 * KIB, now);
        // 100 bytes at 1 MiB/s: about 95 µs, not slept
        assert_eq!(block.take(client(1), 100, now), None);
        // Owed with the next part
        assert!(block.take(client(1), KIB, now).is_some());
    }

    #[test]
    fn test_counting_a_known_client_allocates_nothing() {
        let block = limit(serde_json::json!({ "rate": "100KiB" })).unwrap();
        let now = Instant::now();
        crate::allocations::assert_no_allocations("counting a part of a body", || {
            block.take(client(1), KIB, now)
        });
    }

    #[test]
    fn test_refilled_buckets_are_dropped() {
        let block = limit(serde_json::json!({ "rate": "100KiB" })).unwrap();
        let start = Instant::now();
        block.take(client(0), 100 * KIB, start);

        // The sweep runs as the `SWEEP_EVERY`th new client comes
        let later = start + Duration::from_secs(10);
        let others = u32::try_from(SWEEP_EVERY - 1).unwrap();
        for other in 1..=others {
            block.take(IpAddr::from((0xc612_0000 + other).to_be_bytes()), 1, later);
        }
        // Client 0 refilled long ago; the others aren't full yet
        assert!(block.buckets.pin().get(&client(0)).is_none());
        assert_eq!(block.buckets.len(), SWEEP_EVERY - 1);
    }
}
