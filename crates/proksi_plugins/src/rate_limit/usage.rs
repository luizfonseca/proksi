//! Requests each client made since the last sync, so replicas can share them (see `sync`). The
//! request path never waits or allocates here: a busy shard, or a full one, leaves the client
//! decided locally only, until the next sync.

use std::{
    collections::HashMap,
    hash::{BuildHasherDefault, Hasher},
    net::IpAddr,
    sync::{
        Mutex, PoisonError, TryLockError,
        atomic::{AtomicBool, Ordering},
    },
};

/// Clients are spread over this many maps, each with its own lock
const SHARDS: usize = 64;

/// Clients each shard holds: a map with room for 14 takes 16 buckets, and holds exactly 14
const PER_SHARD: usize = 14;

/// Clients a block counts between two syncs; more are decided locally only until the next
#[cfg(test)]
pub(crate) const CLIENTS: usize = SHARDS * PER_SHARD;

/// A shard's clients. Their hasher needn't resist collisions: a shard holds 14 clients at most,
/// so the worst a crafted address can do is a probe of 14.
type Clients = HashMap<IpAddr, u64, BuildHasherDefault<Mix>>;

#[derive(Default)]
struct Mix(u64);

impl Hasher for Mix {
    fn write(&mut self, bytes: &[u8]) {
        for byte in bytes {
            self.0 = (self.0.rotate_left(5) ^ u64::from(*byte)).wrapping_mul(0x517c_c1b7_2722_0a95);
        }
    }

    fn finish(&self) -> u64 {
        self.0
    }
}

/// Each shard holds at most its map's capacity, which only `make_room` (in the background)
/// sets, once the block is used: until then, nothing is counted, and nothing is allocated
pub(crate) struct Usage {
    shards: Box<[Mutex<Clients>]>,
    /// Whether a request found no room: the next sync makes it
    wanted: AtomicBool,
}

impl Usage {
    pub(crate) fn new() -> Self {
        Self {
            shards: (0..SHARDS).map(|_| Mutex::default()).collect(),
            wanted: AtomicBool::new(false),
        }
    }

    /// Makes room for `CLIENTS` between two syncs, once a request asked for it
    pub(crate) fn make_room(&self) {
        if !self.wanted.swap(false, Ordering::Relaxed) {
            return;
        }
        for shard in &self.shards {
            let mut clients = shard.lock().unwrap_or_else(PoisonError::into_inner);
            let missing = PER_SHARD.saturating_sub(clients.capacity());
            clients.reserve(missing);
        }
    }

    /// Clients the shards have room for
    #[cfg(test)]
    pub(crate) fn room(&self) -> usize {
        let room = |shard: &Mutex<Clients>| shard.lock().map_or(0, |c| c.capacity());
        self.shards.iter().map(room).sum()
    }

    /// Counts a request of `client`, hashed with `hash` (its high bits pick the shard), unless
    /// its shard is busy or full
    pub(crate) fn record(&self, client: IpAddr, hash: u64) {
        let index = usize::try_from(hash >> 58).unwrap_or(0) % SHARDS;
        let Some(shard) = self.shards.get(index) else {
            return;
        };
        let mut clients = match shard.try_lock() {
            Ok(clients) => clients,
            Err(TryLockError::Poisoned(poisoned)) => poisoned.into_inner(),
            Err(TryLockError::WouldBlock) => return,
        };
        if let Some(requests) = clients.get_mut(&client) {
            *requests += 1;
        } else if clients.len() < clients.capacity() {
            clients.insert(client, 1);
        } else if clients.capacity() == 0 {
            self.wanted.store(true, Ordering::Relaxed);
        }
    }

    /// The requests counted since the last time, by client
    pub(crate) fn take(&self) -> Vec<(IpAddr, u64)> {
        self.shards
            .iter()
            .flat_map(|shard| {
                let mut clients = shard.lock().unwrap_or_else(PoisonError::into_inner);
                clients.drain().collect::<Vec<_>>()
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use std::net::IpAddr;

    use std::hash::{BuildHasher, BuildHasherDefault, DefaultHasher};

    use super::Usage;

    /// Records `client` with a hash like the Gcra's (the same for the same client)
    fn record(usage: &Usage, client: IpAddr) {
        let hash = BuildHasherDefault::<DefaultHasher>::default().hash_one(client);
        usage.record(client, hash);
    }

    fn ip(last: u8) -> IpAddr {
        IpAddr::from([10, 0, 0, last])
    }

    #[test]
    fn test_usage_counts_each_client() {
        let usage = Usage::new();
        record(&usage, ip(0));
        usage.make_room();
        for _ in 0..3 {
            record(&usage, ip(1));
        }
        record(&usage, ip(2));
        let mut taken = usage.take();
        taken.sort();
        assert_eq!(taken, vec![(ip(1), 3), (ip(2), 1)]);
        assert!(usage.take().is_empty());
    }

    #[test]
    fn test_nothing_is_counted_without_room() {
        let usage = Usage::new();
        crate::allocations::assert_no_allocations("recording without room", || {
            record(&usage, ip(1));
        });
        assert!(usage.take().is_empty());
    }

    #[test]
    fn test_room_is_only_made_for_used_blocks() {
        let usage = Usage::new();
        usage.make_room();
        assert_eq!(usage.room(), 0);
        // A request with no room asks for it, for the next sync
        record(&usage, ip(1));
        usage.make_room();
        assert_eq!(usage.room(), super::CLIENTS);
    }

    #[test]
    fn test_usage_is_bounded() {
        let usage = Usage::new();
        record(&usage, ip(0));
        usage.make_room();
        let clients = |n: u16| (0..n).map(|i| IpAddr::from([10, 1, (i >> 8) as u8, i as u8]));
        clients(4000).for_each(|client| record(&usage, client));
        let counted = usage.take().len();
        assert!(counted > 0 && counted <= super::CLIENTS, "{counted}");
        clients(4000).for_each(|client| record(&usage, client));
        crate::allocations::assert_no_allocations("recording a new client in a full shard", || {
            clients(4000).for_each(|client| record(&usage, client));
        });
    }
}
