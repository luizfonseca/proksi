use std::{
    collections::HashMap,
    sync::Arc,
    time::{Duration, Instant},
};

use async_trait::async_trait;
use pingora::{
    lb::{selection::RoundRobin, LoadBalancer},
    server::{ListenFds, ShutdownWatch},
    services::Service,
};

use crate::stores::{self};

/// Health check service that will run health checks on all upstreams
/// And update the route store with the new healthy upstreams.
/// This service will run in a separate thread.
pub struct HealthService {}

impl HealthService {
    pub fn new() -> Self {
        Self {}
    }
}

/// How often upstream names are resolved again (ADR 0006)
const RESOLVE_INTERVAL: Duration = Duration::from_secs(5);

/// The load balancers of every location and `tcp` block, shared with the route table: health
/// checks and new addresses apply to the routes being served
fn load_balancers() -> Vec<(String, Arc<LoadBalancer<RoundRobin>>)> {
    stores::get_routes()
        .iter()
        .flat_map(|(host, container)| {
            container
                .locations
                .iter()
                .flat_map(|location| {
                    location
                        .load_balancers()
                        .map(|load_balancer| (host.clone(), load_balancer.clone()))
                        .collect::<Vec<_>>()
                })
                .collect::<Vec<_>>()
        })
        .chain(crate::tcp::load_balancers().iter().cloned())
        .collect()
}

/// Checks the upstreams of each load balancer on its own interval (`health_check_frequency`,
/// unset when its checks are off)
async fn run_health_check_loop() {
    let mut interval = tokio::time::interval(Duration::from_secs(1));
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    // When each load balancer (by address) was last checked
    let mut checked_at = HashMap::<usize, Instant>::new();

    loop {
        interval.tick().await;

        let load_balancers = load_balancers();
        checked_at.retain(|id, _| {
            load_balancers
                .iter()
                .any(|(_, load_balancer)| Arc::as_ptr(load_balancer) as usize == *id)
        });
        for (host, load_balancer) in load_balancers {
            let Some(frequency) = load_balancer.health_check_frequency else {
                continue;
            };
            let last = checked_at.entry(Arc::as_ptr(&load_balancer) as usize);
            let last = last.or_insert_with(Instant::now);
            if last.elapsed() < frequency {
                continue;
            }
            *last = Instant::now();
            tracing::trace!("Running health check for host {}", host);
            load_balancer.backends().run_health_check(false).await;
        }

        record_health();
    }
}

/// Resolves upstream names again, replacing their addresses when they changed
async fn run_resolve_loop() {
    let mut interval = tokio::time::interval(RESOLVE_INTERVAL);
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    interval.tick().await;

    loop {
        interval.tick().await;
        for (_, load_balancer) in load_balancers() {
            load_balancer.update().await.ok();
        }
    }
}

/// Sets `proksi_upstream_healthy` from the health checks: an upstream is healthy when one of
/// its addresses is
fn record_health() {
    for (_, host) in &stores::get_routes() {
        for location in &host.locations {
            let Some(metrics) = &location.metrics else {
                continue;
            };
            for (index, healthy) in location.upstream_health().into_iter().enumerate() {
                if let Some(upstream) = metrics.upstream(index) {
                    upstream.set_healthy(healthy);
                }
            }
        }
    }
}

#[async_trait]
impl Service for HealthService {
    async fn start_service(
        &mut self,
        _fds: Option<ListenFds>,
        _shutdown: ShutdownWatch,
        _listeners_per_fd: usize,
    ) {
        tracing::info!("Starting health check service");

        tokio::join!(run_health_check_loop(), run_resolve_loop());
    }

    fn name(&self) -> &'static str {
        "health_check_service"
    }

    fn threads(&self) -> Option<usize> {
        Some(1)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use crate::{
        routing::{build::build_host_container, test_utils::host},
        stores,
    };

    #[test]
    fn test_upstream_health_metrics() {
        crate::metrics::enable();
        let container = build_host_container(&host("health-metrics.test", "10.0.0.9")).unwrap();
        let location = container.locations[0].clone();
        stores::insert_route("health-metrics.test".to_string(), Arc::new(container));
        let healthy = || {
            crate::metrics::gather().contains(
                "proksi_upstream_healthy{host=\"health-metrics.test\",location=\"/*\",upstream=\"10.0.0.9:3000\"} 1",
            )
        };
        assert!(healthy());

        // A health check failed
        let backends = location.load_balancer.backends();
        let backend = backends.get_backend().iter().next().unwrap().clone();
        backends.set_enable(&backend, false);
        super::record_health();
        assert!(!healthy());

        backends.set_enable(&backend, true);
        super::record_health();
        assert!(healthy());
        stores::remove_route("health-metrics.test");
    }
}
