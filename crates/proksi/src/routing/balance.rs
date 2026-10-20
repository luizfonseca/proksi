//! Choosing a request's upstream: in turn, in proportion to their weight (round robin), or the
//! one with the fewest requests in progress for its weight (least connections). A request tried
//! again goes to an upstream it wasn't sent to yet, while one is healthy. Nothing is allocated:
//! the backends are read where the load balancer keeps them, and only what the request needs of
//! the chosen one is copied.

use std::{
    collections::BTreeSet,
    net::SocketAddr,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
};

use pingora::lb::{selection::RoundRobin, Backend, LoadBalancer};

use crate::{
    config::host::LoadBalancing,
    routing::build::{in_flight, upstream_index},
};

/// The upstreams a request was sent to, by index among its location's (the first 64)
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Tried(u64);

impl Tried {
    pub fn add(&mut self, index: usize) {
        if let Some(bit) = 1u64.checked_shl(u32::try_from(index).unwrap_or(u32::MAX)) {
            self.0 |= bit;
        }
    }

    fn holds(self, backend: &Backend) -> bool {
        upstream_index(backend)
            .and_then(|index| 1u64.checked_shl(u32::try_from(index).ok()?))
            .is_some_and(|bit| self.0 & bit != 0)
    }
}

/// Whose turn it is among a load balancer's backends (round robin), and where the search for
/// the least loaded one starts, so ties are taken in turn
#[derive(Debug, Default)]
pub struct Turn(AtomicUsize);

impl Turn {
    fn next(&self) -> usize {
        self.0.fetch_add(1, Ordering::Relaxed)
    }
}

/// The backend chosen for a request: its address, the upstream it came from, and its requests
/// in progress (least connections)
#[derive(Debug)]
pub struct Picked {
    pub addr: SocketAddr,
    pub index: Option<usize>,
    pub in_flight: Option<Arc<AtomicUsize>>,
}

impl Picked {
    fn of(backend: &Backend) -> Option<Self> {
        Some(Self {
            addr: *backend.addr.as_inet()?,
            index: upstream_index(backend),
            in_flight: in_flight(backend).cloned(),
        })
    }
}

/// A healthy backend, by `balancing`: of an upstream not `tried` yet, else of any
pub fn pick(
    load_balancer: &LoadBalancer<RoundRobin>,
    turn: &Turn,
    balancing: LoadBalancing,
    tried: Tried,
) -> Option<Picked> {
    let backends = load_balancer.backends();
    let all = backends.get_backend();
    let healthy = |backend: &Backend| backends.ready(backend);
    let untried = |backend: &Backend| healthy(backend) && !tried.holds(backend);
    let chosen = match balancing {
        LoadBalancing::RoundRobin => {
            round_robin(&all, turn, untried).or_else(|| round_robin(&all, turn, healthy))
        }
        LoadBalancing::LeastConnections => least_connections(&all, turn, untried)
            .or_else(|| least_connections(&all, turn, healthy)),
    };
    chosen.and_then(Picked::of)
}

/// Like [`pick`], resolving the upstreams' names again when none is healthy, then trying them
/// anyway (see [`crate::routing::build::select_or_resolve`])
pub async fn pick_or_resolve(
    load_balancer: &LoadBalancer<RoundRobin>,
    turn: &Turn,
    balancing: LoadBalancing,
    tried: Tried,
) -> Option<Picked> {
    if let Some(picked) = pick(load_balancer, turn, balancing, tried) {
        return Some(picked);
    }
    let _ = load_balancer.update().await;
    if let Some(picked) = pick(load_balancer, turn, balancing, tried) {
        return Some(picked);
    }
    let all = load_balancer.backends().get_backend();
    round_robin(&all, turn, |_| true).and_then(Picked::of)
}

/// The backend whose turn it is, each taking turns in proportion to its weight, or the next one
/// `accept`ed after it
fn round_robin<'a>(
    all: &'a BTreeSet<Backend>,
    turn: &Turn,
    accept: impl Fn(&Backend) -> bool,
) -> Option<&'a Backend> {
    let total: usize = all.iter().map(|backend| backend.weight.max(1)).sum();
    if total == 0 {
        return None;
    }
    let mut slot = turn.next() % total;
    let start = all.iter().position(|backend| {
        let weight = backend.weight.max(1);
        let here = slot < weight;
        slot = slot.saturating_sub(weight);
        here
    })?;
    all.iter()
        .cycle()
        .skip(start)
        .take(all.len())
        .find(|backend| accept(backend))
}

/// The backend `accept`ed with the fewest requests in progress for its weight
fn least_connections<'a>(
    all: &'a BTreeSet<Backend>,
    turn: &Turn,
    accept: impl Fn(&Backend) -> bool,
) -> Option<&'a Backend> {
    if all.is_empty() {
        return None;
    }
    let start = turn.next() % all.len();
    let mut best: Option<(&Backend, usize)> = None;
    for backend in all.iter().cycle().skip(start).take(all.len()) {
        if !accept(backend) {
            continue;
        }
        let load = in_flight(backend).map_or(0, |count| count.load(Ordering::Relaxed));
        // Fewer per unit of weight: load / weight < best / its weight
        let fewer = best.is_none_or(|(chosen, chosen_load)| {
            (load as u128) * (chosen.weight.max(1) as u128)
                < (chosen_load as u128) * (backend.weight.max(1) as u128)
        });
        if fewer {
            best = Some((backend, load));
        }
    }
    best.map(|(backend, _)| backend)
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::Ordering;

    use crate::config::{host::LoadBalancing, RouteUpstream};
    use crate::routing::build::{in_flight, load_balancer, upstream_index};

    use super::{pick, Tried, Turn};

    /// One turn for the tests' picks, as a location has
    static TURN: Turn = Turn(std::sync::atomic::AtomicUsize::new(0));

    fn upstreams(weights: &[i8]) -> Vec<RouteUpstream> {
        weights
            .iter()
            .enumerate()
            .map(|(index, weight)| RouteUpstream {
                ip: format!("10.0.0.{}", index + 1).into(),
                port: 80,
                weight: Some(*weight),
                ..RouteUpstream::default()
            })
            .collect()
    }

    fn index(
        load_balancer: &pingora::lb::LoadBalancer<pingora::lb::selection::RoundRobin>,
        balancing: LoadBalancing,
        tried: Tried,
    ) -> usize {
        pick(load_balancer, &TURN, balancing, tried)
            .unwrap()
            .index
            .unwrap()
    }

    #[test]
    fn test_least_connections_picks_the_least_loaded() {
        let load_balancer = load_balancer(&upstreams(&[1, 1, 1]));
        let backends = load_balancer.backends().get_backend();
        let busy = |index: usize, count: usize| {
            let backend = backends
                .iter()
                .find(|backend| upstream_index(backend) == Some(index))
                .unwrap();
            in_flight(backend).unwrap().store(count, Ordering::Relaxed);
        };
        busy(0, 5);
        busy(1, 1);
        busy(2, 3);
        for _ in 0..5 {
            assert_eq!(
                index(
                    &load_balancer,
                    LoadBalancing::LeastConnections,
                    Tried::default()
                ),
                1
            );
        }

        // Ties are taken in turn
        busy(2, 1);
        let picked = (0..20)
            .map(|_| {
                index(
                    &load_balancer,
                    LoadBalancing::LeastConnections,
                    Tried::default(),
                )
            })
            .collect::<std::collections::BTreeSet<_>>();
        assert_eq!(picked, [1, 2].into());
    }

    #[test]
    fn test_least_connections_weighs_upstreams() {
        let load_balancer = load_balancer(&upstreams(&[1, 4]));
        let backends = load_balancer.backends().get_backend();
        for backend in backends.iter() {
            // 2 in progress for weight 1, 4 for weight 4: the second is less loaded
            let count = if upstream_index(backend) == Some(0) {
                2
            } else {
                4
            };
            in_flight(backend).unwrap().store(count, Ordering::Relaxed);
        }
        assert_eq!(
            index(
                &load_balancer,
                LoadBalancing::LeastConnections,
                Tried::default()
            ),
            1
        );
    }

    #[test]
    fn test_retries_go_to_upstreams_not_tried() {
        let load_balancer = load_balancer(&upstreams(&[1, 1, 1]));
        for balancing in [LoadBalancing::RoundRobin, LoadBalancing::LeastConnections] {
            let mut tried = Tried::default();
            tried.add(0);
            tried.add(2);
            for _ in 0..10 {
                assert_eq!(index(&load_balancer, balancing, tried), 1, "{balancing:?}");
            }
            // All tried: any
            tried.add(1);
            assert!(pick(&load_balancer, &TURN, balancing, tried).is_some());
        }
    }
}
