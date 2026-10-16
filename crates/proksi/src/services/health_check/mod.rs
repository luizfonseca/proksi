use std::time::Duration;

use async_trait::async_trait;
use pingora::{
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

async fn run_health_check_loop() {
    let mut interval = tokio::time::interval(Duration::from_secs(30));
    interval.tick().await;

    loop {
        interval.tick().await;

        // The load balancers are shared with the route table, so health checks
        // update the routes being served
        let load_balancers = stores::get_routes()
            .iter()
            .flat_map(|(host, container)| {
                container
                    .locations
                    .iter()
                    .map(|location| (host.clone(), location.load_balancer.clone()))
                    .collect::<Vec<_>>()
            })
            .chain(crate::tcp::load_balancers().iter().cloned())
            .collect::<Vec<_>>();

        for (host, load_balancer) in load_balancers {
            tracing::trace!("Running health check for host {}", host);
            load_balancer.update().await.ok();
            load_balancer.backends().run_health_check(false).await;
        }

        record_health();
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
            let backends = location.load_balancer.backends();
            let mut healthy = vec![false; location.upstreams.len()];
            for backend in backends.get_backend().iter() {
                let index = backend
                    .addr
                    .as_inet()
                    .and_then(|addr| location.upstream_index(addr));
                if let Some(slot) = index.and_then(|index| healthy.get_mut(index)) {
                    *slot |= backends.ready(backend);
                }
            }
            for (index, healthy) in healthy.into_iter().enumerate() {
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

        run_health_check_loop().await;
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
