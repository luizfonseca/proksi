//! Rate limits shared by replicas: each sends how many requests its clients made since it last
//! did, and learns how far ahead each client's arrival time is (GCRA, see `proksi_plugins`)

use std::{collections::HashMap, time::Instant};

/// A block's rate, in microseconds
#[derive(Debug, Clone, Copy)]
pub struct Rate {
    /// Between two requests
    pub interval_us: u64,
    /// How far ahead of now a client's arrival time may be
    pub capacity_us: u64,
}

/// Requests a client made, as a replica counted them
#[derive(Debug, Clone)]
pub struct Used {
    pub client: String,
    pub requests: u64,
}

/// A client's arrival time once `requests` are counted at `now`, as Redis' script does
pub(crate) fn arrival(stored: u64, now: u64, interval_us: u64, requests: u64) -> u64 {
    stored
        .max(now)
        .saturating_add(requests.saturating_mul(interval_us.max(1)))
}

/// Arrival times kept in memory, by block and client: one process, so nothing is shared, but
/// it counts the same way Redis does
#[derive(Default)]
pub(crate) struct Arrivals {
    times: HashMap<(String, String), u64>,
    start: Option<Instant>,
}

impl Arrivals {
    pub(crate) fn count(&mut self, block: &str, rate: Rate, used: &[Used]) -> Vec<u64> {
        let start = *self.start.get_or_insert_with(Instant::now);
        let now = u64::try_from(start.elapsed().as_micros()).unwrap_or(u64::MAX);
        // Clients whose arrival time passed are as good as new
        self.times.retain(|_, arrival| *arrival > now);
        used.iter()
            .map(|used| {
                let key = (block.to_string(), used.client.clone());
                let stored = self.times.get(&key).copied().unwrap_or(0);
                let next = arrival(stored, now, rate.interval_us, used.requests);
                self.times.insert(key, next);
                next - now
            })
            .collect()
    }
}
