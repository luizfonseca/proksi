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
            .map(|(host, route)| (host.clone(), route.load_balancer.clone()))
            .collect::<Vec<_>>();

        for (host, load_balancer) in load_balancers {
            tracing::trace!("Running health check for host {}", host);
            load_balancer.update().await.ok();
            load_balancer.backends().run_health_check(false).await;
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
