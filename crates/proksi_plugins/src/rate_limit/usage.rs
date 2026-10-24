//! Requests each client made since the last sync, so replicas can share them (see `sync`). The
//! request path never waits or allocates here: a busy shard, or a full one, leaves the client
//! decided locally only, until the next sync.

use std::{
    collections::HashMap,
    hash::{BuildHasher, RandomState},
    net::IpAddr,
    sync::{Mutex, PoisonError, TryLockError},
};

/// Clients are spread over this many maps, each with its own lock
const SHARDS: usize = 16;

/// Each shard holds at most its map's capacity, which only `reserve` (in the background) sets:
/// without it, nothing is counted, and nothing is allocated
pub(crate) struct Usage {
    shards: Box<[Mutex<HashMap<IpAddr, u64>>]>,
    hasher: RandomState,
}

impl Usage {
    pub(crate) fn new() -> Self {
        Self {
            shards: (0..SHARDS).map(|_| Mutex::new(HashMap::new())).collect(),
            hasher: RandomState::new(),
        }
    }

    /// Makes room for `clients` between two syncs
    pub(crate) fn reserve(&self, clients: usize) {
        let per_shard = (clients / SHARDS).max(1);
        for shard in &self.shards {
            let mut clients = shard.lock().unwrap_or_else(PoisonError::into_inner);
            let missing = per_shard.saturating_sub(clients.capacity());
            clients.reserve(missing);
        }
    }

    /// Counts a request of `client`, unless its shard is busy or full
    pub(crate) fn record(&self, client: IpAddr) {
        let index = usize::try_from(self.hasher.hash_one(client)).unwrap_or(0) % SHARDS;
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

    use super::Usage;

    fn ip(last: u8) -> IpAddr {
        IpAddr::from([10, 0, 0, last])
    }

    #[test]
    fn test_usage_counts_each_client() {
        let usage = Usage::new();
        usage.reserve(64);
        for _ in 0..3 {
            usage.record(ip(1));
        }
        usage.record(ip(2));
        let mut taken = usage.take();
        taken.sort();
        assert_eq!(taken, vec![(ip(1), 3), (ip(2), 1)]);
        assert!(usage.take().is_empty());
    }

    #[test]
    fn test_nothing_is_counted_without_room() {
        let usage = Usage::new();
        crate::allocations::assert_no_allocations("recording without room", || {
            usage.record(ip(1));
        });
        assert!(usage.take().is_empty());
    }

    #[test]
    fn test_usage_is_bounded() {
        // About one client per shard
        let usage = Usage::new();
        usage.reserve(16);
        (0..=255).for_each(|last| usage.record(ip(last)));
        let capacity: usize = usage
            .shards
            .iter()
            .map(|shard| shard.lock().unwrap().capacity())
            .sum();
        assert!(usage.take().len() <= capacity);
        (0..=255).for_each(|last| usage.record(ip(last)));
        crate::allocations::assert_no_allocations("recording a new client in a full shard", || {
            (0..=255).for_each(|last| usage.record(ip(last)));
        });
    }
}
