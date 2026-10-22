//! Holds back events that come in pairs (down, then up): a state is sent when it changes, at
//! most once a `window` for the same thing, so a flapping upstream doesn't flood a webhook.
//! What changed meanwhile is sent when the window ends, if it differs from what was sent.

use std::{
    collections::HashMap,
    time::{Duration, Instant},
};

use super::Event;
use crate::config::notify::Kind;

/// The other half of a pair, for kinds that come in pairs
fn opposite(kind: Kind) -> Option<Kind> {
    match kind {
        Kind::UpstreamDown => Some(Kind::UpstreamUp),
        Kind::UpstreamUp => Some(Kind::UpstreamDown),
        Kind::StoreDown => Some(Kind::StoreUp),
        Kind::StoreUp => Some(Kind::StoreDown),
        _ => None,
    }
}

/// Whether `kind` says something recovered
fn recovery(kind: Kind) -> bool {
    matches!(kind, Kind::UpstreamUp | Kind::StoreUp)
}

struct Held {
    sent: Kind,
    sent_at: Instant,
    /// The latest event since, held until the window ends
    pending: Option<Event>,
}

pub struct Hold {
    window: Duration,
    subjects: HashMap<String, Held>,
}

impl Hold {
    pub fn new(window: Duration) -> Self {
        Self {
            window,
            subjects: HashMap::new(),
        }
    }

    /// The event to send now, if any (events that don't come in pairs are always sent)
    pub fn offer(&mut self, event: Event, now: Instant) -> Option<Event> {
        if opposite(event.kind).is_none() {
            return Some(event);
        }
        let subject = event.subject();
        let Some(held) = self.subjects.get_mut(&subject) else {
            // Something recovering that was never said to be down: nothing to tell
            if recovery(event.kind) {
                return None;
            }
            self.subjects.insert(
                subject,
                Held {
                    sent: event.kind,
                    sent_at: now,
                    pending: None,
                },
            );
            return Some(event);
        };
        if now.duration_since(held.sent_at) < self.window {
            held.pending = Some(event);
            return None;
        }
        if held.sent == event.kind {
            return None;
        }
        held.sent = event.kind;
        held.sent_at = now;
        held.pending = None;
        Some(event)
    }

    /// The events held back whose window ended, and that change what was sent
    pub fn due(&mut self, now: Instant) -> Vec<Event> {
        let mut due = vec![];
        for held in self.subjects.values_mut() {
            if now.duration_since(held.sent_at) < self.window {
                continue;
            }
            let Some(pending) = held.pending.take() else {
                continue;
            };
            if pending.kind != held.sent {
                held.sent = pending.kind;
                held.sent_at = now;
                due.push(pending);
            }
        }
        due
    }
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use super::Hold;
    use crate::{config::notify::Kind, notify::Event};

    fn upstream(kind: Kind) -> Event {
        Event {
            host: Some("api.example.com".into()),
            location: Some("/".into()),
            upstream: Some("10.0.0.1:80".into()),
            ..Event::new(kind)
        }
    }

    fn kinds(events: &[Event]) -> Vec<Kind> {
        events.iter().map(|event| event.kind).collect()
    }

    #[test]
    fn test_changes_are_sent_once() {
        let mut hold = Hold::new(Duration::from_secs(60));
        let start = Instant::now();
        // An upstream that was never down isn't said to be up
        assert!(hold.offer(upstream(Kind::UpstreamUp), start).is_none());
        assert!(hold.offer(upstream(Kind::UpstreamDown), start).is_some());
        // Back up, a while later: sent
        let later = start + Duration::from_secs(120);
        assert!(hold.offer(upstream(Kind::UpstreamUp), later).is_some());
        // Again up: nothing changed
        assert!(
            hold.offer(upstream(Kind::UpstreamUp), later + Duration::from_secs(120))
                .is_none()
        );
        // Events that don't come in pairs are always sent
        assert!(
            hold.offer(Event::new(Kind::ConfigReloaded), start)
                .is_some()
        );
        assert!(
            hold.offer(Event::new(Kind::ConfigReloaded), start)
                .is_some()
        );
    }

    #[test]
    fn test_flapping_is_held_back() {
        let mut hold = Hold::new(Duration::from_secs(60));
        let start = Instant::now();
        assert!(hold.offer(upstream(Kind::UpstreamDown), start).is_some());
        // Up and down again within the window: held, then nothing to send (still down)
        assert!(
            hold.offer(upstream(Kind::UpstreamUp), start + Duration::from_secs(5))
                .is_none()
        );
        assert!(
            hold.offer(
                upstream(Kind::UpstreamDown),
                start + Duration::from_secs(10)
            )
            .is_none()
        );
        assert!(hold.due(start + Duration::from_secs(30)).is_empty());
        assert!(hold.due(start + Duration::from_secs(61)).is_empty());

        // Up within the window, and staying up: sent when the window ends
        let next = start + Duration::from_secs(200);
        assert!(hold.offer(upstream(Kind::UpstreamUp), next).is_some());
        assert!(
            hold.offer(upstream(Kind::UpstreamDown), next + Duration::from_secs(5))
                .is_none()
        );
        assert!(hold.due(next + Duration::from_secs(30)).is_empty());
        assert_eq!(
            kinds(&hold.due(next + Duration::from_secs(61))),
            vec![Kind::UpstreamDown]
        );
        assert!(hold.due(next + Duration::from_secs(200)).is_empty());
    }

    #[test]
    fn test_things_are_held_apart() {
        let mut hold = Hold::new(Duration::from_secs(60));
        let start = Instant::now();
        let other = Event {
            upstream: Some("10.0.0.2:80".into()),
            ..upstream(Kind::UpstreamDown)
        };
        assert!(hold.offer(upstream(Kind::UpstreamDown), start).is_some());
        assert!(hold.offer(other, start).is_some());
        assert!(hold.offer(Event::new(Kind::StoreDown), start).is_some());
    }
}
