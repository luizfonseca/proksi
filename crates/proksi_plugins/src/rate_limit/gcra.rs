//! GCRA (generic cell rate algorithm): each client has a "theoretical arrival time", which each
//! request it makes moves `interval` later. A request is let through while that time is at most
//! `capacity` ahead of now: `requests + burst` at once, then one every `interval`.

use std::{
    hash::{BuildHasher, RandomState},
    net::IpAddr,
    sync::atomic::{AtomicU64, Ordering},
    time::{Duration, Instant},
};

/// Slots of the table: clients are spread over them by a hash of their address
const SLOTS: usize = 4096;

/// The arrival times of clients, in a fixed-size table. One row: a count-min style minimum over
/// several rows would let racing requests through, each seeing room in a different row. Clients
/// sharing a slot add up (limited early, never late).
pub(crate) struct Gcra {
    /// Nanoseconds between two requests
    interval: u64,
    /// How far ahead of now a client's arrival time may be, in nanoseconds
    capacity: u64,
    /// Nanoseconds since `start`, 0 for none yet
    slots: Box<[AtomicU64]>,
    hasher: RandomState,
    start: Instant,
}

impl Gcra {
    /// `requests` per `per`, and `burst` more at once
    pub(crate) fn new(requests: u64, per: Duration, burst: u64) -> Self {
        Self::with_slots(requests, per, burst, SLOTS)
    }

    pub(crate) fn with_slots(requests: u64, per: Duration, burst: u64, slots: usize) -> Self {
        let per = u64::try_from(per.as_nanos()).unwrap_or(u64::MAX);
        let interval = (per / requests.max(1)).max(1);
        Self {
            interval,
            capacity: interval.saturating_mul(requests.saturating_add(burst)),
            slots: (0..slots.max(1)).map(|_| AtomicU64::new(0)).collect(),
            hasher: RandomState::new(),
            start: Instant::now(),
        }
    }

    /// Now, as `check` takes it
    pub(crate) fn now(&self) -> u64 {
        u64::try_from(self.start.elapsed().as_nanos()).unwrap_or(u64::MAX)
    }

    /// Counts a request of `client` at `now` (see `now`) if it's let through, or says how long
    /// until one would be
    pub(crate) fn check(&self, client: &IpAddr, now: u64) -> Result<(), Duration> {
        let Some(slot) = self.slot(client) else {
            return Ok(());
        };
        let interval = self.interval;
        let previous = slot.update(Ordering::Relaxed, Ordering::Relaxed, |arrival| {
            arrival.max(now).saturating_add(interval)
        });
        let ahead = previous.max(now).saturating_add(interval) - now;
        if ahead <= self.capacity {
            return Ok(());
        }
        // Refused requests don't count
        slot.update(Ordering::Relaxed, Ordering::Relaxed, |arrival| {
            arrival.saturating_sub(interval)
        });
        Err(Duration::from_nanos(ahead - self.capacity))
    }

    fn slot(&self, client: &IpAddr) -> Option<&AtomicU64> {
        let slots = u64::try_from(self.slots.len()).ok()?;
        let index = usize::try_from(self.hasher.hash_one(client) % slots).ok()?;
        self.slots.get(index)
    }
}

#[cfg(test)]
mod tests {
    use std::{
        net::IpAddr,
        sync::atomic::{AtomicUsize, Ordering},
        time::Duration,
    };

    use super::Gcra;

    const SECOND: u64 = 1_000_000_000;

    fn ip(last: u8) -> IpAddr {
        IpAddr::from([10, 0, 0, last])
    }

    #[test]
    fn test_requests_at_once_then_refill() {
        // 2 per second, no burst: 2 at once, then one every 500ms
        let gcra = Gcra::new(2, Duration::from_secs(1), 0);
        assert!(gcra.check(&ip(1), 0).is_ok());
        assert!(gcra.check(&ip(1), 0).is_ok());
        assert_eq!(gcra.check(&ip(1), 0), Err(Duration::from_millis(500)));
        assert!(gcra.check(&ip(1), SECOND / 2).is_ok());
        assert!(gcra.check(&ip(1), SECOND / 2).is_err());
    }

    #[test]
    fn test_burst_is_added_to_requests() {
        let gcra = Gcra::new(100, Duration::from_secs(60), 50);
        let allowed = (0..200).filter(|_| gcra.check(&ip(1), 0).is_ok()).count();
        assert_eq!(allowed, 150);
    }

    #[test]
    fn test_each_client_has_its_own_limit() {
        let gcra = Gcra::new(1, Duration::from_secs(60), 0);
        assert!(gcra.check(&ip(1), 0).is_ok());
        assert!(gcra.check(&ip(1), 0).is_err());
        assert!(gcra.check(&ip(2), 0).is_ok());
    }

    #[test]
    fn test_rejected_requests_dont_count() {
        // 1 per second: hammering 10 times a second still gets 1 a second through
        let gcra = Gcra::new(1, Duration::from_secs(1), 0);
        let allowed = (0..100u64)
            .filter(|tick| gcra.check(&ip(1), tick * SECOND / 10).is_ok())
            .count();
        assert_eq!(allowed, 10);
    }

    #[test]
    fn test_collisions_only_limit_early() {
        // One slot: every client shares it
        let gcra = Gcra::with_slots(3, Duration::from_secs(60), 0, 1);
        for client in 1..=3 {
            assert!(gcra.check(&ip(client), 0).is_ok());
        }
        assert!(gcra.check(&ip(1), 0).is_err());
        assert!(gcra.check(&ip(4), 0).is_err());
    }

    #[test]
    fn test_concurrent_requests_never_exceed_capacity() {
        let gcra = Gcra::new(10, Duration::from_secs(3600), 5);
        let allowed = AtomicUsize::new(0);
        std::thread::scope(|scope| {
            for _ in 0..8 {
                scope.spawn(|| {
                    for _ in 0..100 {
                        if gcra.check(&ip(1), 0).is_ok() {
                            allowed.fetch_add(1, Ordering::Relaxed);
                        }
                    }
                });
            }
        });
        assert_eq!(allowed.into_inner(), 15);
    }

    #[test]
    fn test_extreme_settings_dont_overflow() {
        let hourly = Gcra::new(1, Duration::from_secs(3600), 0);
        assert!(hourly.check(&ip(1), 0).is_ok());
        assert_eq!(hourly.check(&ip(1), 0), Err(Duration::from_secs(3600)));
        // More requests than nanoseconds in `per`: never limited, no zero interval
        let fast = Gcra::new(u64::MAX, Duration::from_secs(1), 0);
        assert!((0..1000).all(|_| fast.check(&ip(1), 0).is_ok()));
        let huge_burst = Gcra::new(1, Duration::from_secs(1), u64::MAX);
        assert!((0..1000).all(|_| huge_burst.check(&ip(1), u64::MAX / 2).is_ok()));
    }

    #[test]
    fn test_checking_doesnt_allocate() {
        let gcra = Gcra::new(10, Duration::from_secs(1), 0);
        crate::allocations::assert_no_allocations("checking a client", || gcra.check(&ip(1), 0));
    }
}
