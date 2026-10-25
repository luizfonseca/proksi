//! Cursors: times kept by the store, each moved forward by a step at a time, and expiring once
//! the store's now passes them. Replicas sharing a store share them, which is what a rate limit
//! (or any leaky bucket) needs.

use std::{collections::HashMap, time::Instant};

/// Where a cursor is once moved by `step` at `now`: from `now` if it was behind
pub(crate) fn advance(stored: u64, now: u64, step: u64) -> u64 {
    stored.max(now).saturating_add(step)
}

/// Cursors kept in memory, by namespace and key, on their own clock (in microseconds)
#[derive(Default)]
pub(crate) struct Cursors {
    times: HashMap<(String, String), u64>,
    start: Option<Instant>,
}

impl Cursors {
    pub(crate) fn advance(&mut self, namespace: &str, steps: &[(String, u64)]) -> Vec<u64> {
        let start = *self.start.get_or_insert_with(Instant::now);
        let now = u64::try_from(start.elapsed().as_micros()).unwrap_or(u64::MAX);
        // Cursors the clock passed are as good as new
        self.times.retain(|_, time| *time > now);
        steps
            .iter()
            .map(|(key, step)| {
                let key = (namespace.to_string(), key.clone());
                let time = advance(self.times.get(&key).copied().unwrap_or(0), now, *step);
                if time > now {
                    self.times.insert(key, time);
                }
                time - now
            })
            .collect()
    }
}
