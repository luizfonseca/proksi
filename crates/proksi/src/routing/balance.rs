//! Choosing a request's upstream: in turn (round robin), or the one with the fewest requests
//! in progress for its weight (least connections). A request tried again goes to an upstream
//! it wasn't sent to yet, while one is healthy.

use std::sync::atomic::{AtomicUsize, Ordering};

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

/// A healthy backend, by `balancing`: of an upstream not `tried` yet, else of any
pub fn pick(
    load_balancer: &LoadBalancer<RoundRobin>,
    balancing: LoadBalancing,
    tried: Tried,
) -> Option<Backend> {
    match balancing {
        LoadBalancing::RoundRobin if tried == Tried::default() => load_balancer.select(b"", 32),
        LoadBalancing::RoundRobin => load_balancer
            .select_with(b"", 32, |backend, healthy| healthy && !tried.holds(backend))
            .or_else(|| load_balancer.select(b"", 32)),
        LoadBalancing::LeastConnections => {
            least_connections(load_balancer, |backend| !tried.holds(backend))
                .or_else(|| least_connections(load_balancer, |_| true))
        }
    }
}

/// Like [`pick`], resolving the upstreams' names again when none is healthy, then trying them
/// anyway (see [`crate::routing::build::select_or_resolve`])
pub async fn pick_or_resolve(
    load_balancer: &LoadBalancer<RoundRobin>,
    balancing: LoadBalancing,
    tried: Tried,
) -> Option<Backend> {
    if let Some(backend) = pick(load_balancer, balancing, tried) {
        return Some(backend);
    }
    let _ = load_balancer.update().await;
    pick(load_balancer, balancing, tried)
        .or_else(|| load_balancer.select_with(b"", 32, |_, _| true))
}

/// Where the search for the least loaded backend starts, so ties are taken in turn
static NEXT: AtomicUsize = AtomicUsize::new(0);

/// The healthy backend `accept`ed with the fewest requests in progress for its weight
fn least_connections(
    load_balancer: &LoadBalancer<RoundRobin>,
    accept: impl Fn(&Backend) -> bool,
) -> Option<Backend> {
    let backends = load_balancer.backends();
    let all = backends.get_backend();
    if all.is_empty() {
        return None;
    }
    let start = NEXT.fetch_add(1, Ordering::Relaxed) % all.len();
    let mut best: Option<(&Backend, usize)> = None;
    for backend in all.iter().cycle().skip(start).take(all.len()) {
        if !backends.ready(backend) || !accept(backend) {
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
    best.map(|(backend, _)| backend.clone())
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::Ordering;

    use crate::config::{host::LoadBalancing, RouteUpstream};
    use crate::routing::build::{in_flight, load_balancer, upstream_index};

    use super::{pick, Tried};

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
        upstream_index(&pick(load_balancer, balancing, tried).unwrap()).unwrap()
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
            assert!(pick(&load_balancer, balancing, tried).is_some());
        }
    }
}
