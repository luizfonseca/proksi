//! Waiting rooms (the `waiting_room` plugin): which visitors of a room hold one of its slots, and
//! the order the others wait in. Each store keeps them by its own clock, as it keeps leases.

use std::{
    collections::{BTreeMap, HashMap},
    time::Duration,
};

/// How a waiting room lets visitors in
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RoomLimits {
    /// Visitors holding a slot at once
    pub active: u64,
    /// How long a slot is kept after the visitor's last call
    pub session: Duration,
    /// How long a waiting visitor keeps their place without calling again
    pub abandon: Duration,
}

/// Whether a visitor is let in, or where they wait
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Admission {
    /// They hold a slot, until `session` after this call
    Admitted,
    /// Waiting, `position` in line (1 is next), among `waiting`
    Waiting { position: u64, waiting: u64 },
}

/// A waiting room, as the memory store keeps it
#[derive(Debug, Default)]
pub(crate) struct Room {
    next_ticket: u64,
    /// Visitors holding a slot, and when it expires (milliseconds since the epoch)
    active: HashMap<String, u64>,
    /// Waiting visitors, by ticket
    queue: BTreeMap<u64, String>,
    /// Waiting visitors' ticket, and when they last called
    waiting: HashMap<String, (u64, u64)>,
}

impl Room {
    /// Lets `visitor` in if a slot is free for them, or keeps their place, at `now`
    pub(crate) fn wait(&mut self, visitor: &str, limits: RoomLimits, now: u64) -> Admission {
        let session = millis(limits.session);
        self.prune(now, millis(limits.abandon));
        if let Some(expires) = self.active.get_mut(visitor) {
            *expires = now.saturating_add(session);
            return Admission::Admitted;
        }

        let ticket = if let Some((ticket, seen)) = self.waiting.get_mut(visitor) {
            *seen = now;
            *ticket
        } else {
            self.next_ticket += 1;
            self.waiting
                .insert(visitor.to_string(), (self.next_ticket, now));
            self.queue.insert(self.next_ticket, visitor.to_string());
            self.next_ticket
        };
        let free = limits.active.saturating_sub(self.active.len() as u64);
        // Visitors who came before, who get the free slots first
        let before = self.queue.range(..ticket).count() as u64;
        if before >= free {
            return Admission::Waiting {
                position: before - free + 1,
                waiting: self.queue.len() as u64,
            };
        }
        self.queue.remove(&ticket);
        self.waiting.remove(visitor);
        self.active
            .insert(visitor.to_string(), now.saturating_add(session));
        Admission::Admitted
    }

    /// Drops expired slots, and visitors who stopped waiting
    fn prune(&mut self, now: u64, abandon: u64) {
        self.active.retain(|_, expires| *expires > now);
        let gone_before = now.saturating_sub(abandon);
        let queue = &mut self.queue;
        self.waiting.retain(|_, (ticket, seen)| {
            let kept = *seen >= gone_before;
            if !kept {
                queue.remove(ticket);
            }
            kept
        });
    }

    /// Whether nobody is in the room, which can then be dropped
    pub(crate) fn is_empty(&self) -> bool {
        self.active.is_empty() && self.waiting.is_empty()
    }
}

/// `duration` in milliseconds
pub(crate) fn millis(duration: Duration) -> u64 {
    u64::try_from(duration.as_millis()).unwrap_or(u64::MAX)
}

/// A new random key for waiting rooms' cookies
pub(crate) fn new_key() -> Result<[u8; 32], crate::StoreError> {
    let mut key = [0; 32];
    openssl::rand::rand_bytes(&mut key)
        .map_err(|err| crate::StoreError::Unavailable(err.to_string()))?;
    Ok(key)
}

/// `bytes` as hex, as Redis and SQL keep keys
pub(crate) fn hex(bytes: &[u8; 32]) -> String {
    use std::fmt::Write;
    bytes
        .iter()
        .fold(String::with_capacity(64), |mut text, byte| {
            let _ = write!(text, "{byte:02x}");
            text
        })
}

/// A key kept as hex
pub(crate) fn from_hex(text: &str) -> Option<[u8; 32]> {
    let mut key = [0; 32];
    if text.len() != 64 {
        return None;
    }
    for (byte, pair) in key.iter_mut().zip(text.as_bytes().chunks(2)) {
        *byte = u8::from_str_radix(std::str::from_utf8(pair).ok()?, 16).ok()?;
    }
    Some(key)
}
