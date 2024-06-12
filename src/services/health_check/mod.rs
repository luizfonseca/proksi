use std::{collections::HashMap, time::Duration};

use async_trait::async_trait;
use pingora::{
    server::{ListenFds, ShutdownWatch},
    services::Service,
};

use crate::stores::{self};

/// Health check service that will run health checks on all upstreams
/// And update the route store with the new healthy upstreams.
/// This service will run in a separate thread.
pub struct HealthService {
    // route_store: RouteStore,
}

impl HealthService {
    pub fn new() -> Self {
        Self {}
    }
}

async fn run_health_check_loop() {
    let mut interval = tokio::time::interval(Duration::from_secs(20));
    interval.tick().await;

    loop {
        interval.tick().await;
        let mut new_map = HashMap::new();

        for (key, route) in stores::get_routes().iter() {
            tracing::debug!("Running health check for host {}", key);

            let route_container = route.to_owned();

            route_container.load_balancer.update().await.ok();
            route_container
                .load_balancer
                .backends()
                .run_health_check(true)
                .await;

            // TODO: only update if the upstream has changed
            new_map.insert(key.to_owned(), route_container);
        }

        stores::swap_routes(new_map);
    }
}

#[async_trait]
impl Service for HealthService {
    async fn start_service(&mut self, _fds: Option<ListenFds>, _shutdown: ShutdownWatch) {
        tracing::info!("Starting health check service");

        run_health_check_loop().await;
    }

    fn name(&self) -> &str {
        "health_check_service"
    }

    fn threads(&self) -> Option<usize> {
        Some(1)
    }
}
