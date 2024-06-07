use std::{collections::HashMap, time::Duration};

use async_trait::async_trait;
use pingora::{
    server::{ListenFds, ShutdownWatch},
    services::Service,
};

use crate::stores::routes::RouteStore;

/// Health check service that will run health checks on all upstreams
/// And update the route store with the new healthy upstreams.
/// This service will run in a separate thread.
pub struct HealthService {
    route_store: RouteStore,
}

impl HealthService {
    pub fn new(store: RouteStore) -> Self {
        Self { route_store: store }
    }
}

#[async_trait]
impl Service for HealthService {
    async fn start_service(&mut self, _fds: Option<ListenFds>, _shutdown: ShutdownWatch) {
        tracing::info!("Starting health check service");
        // TODO: create multiple interval checks
        let mut interval = tokio::time::interval(Duration::from_secs(15));
        interval.tick().await;

        loop {
            interval.tick().await;
            let mut weak_map = HashMap::new();

            for route in self.route_store.iter() {
                tracing::info!("Running health check for host {}", route.key());
                let route_container = route.clone();
                route_container
                    .load_balancer
                    .backends()
                    .run_health_check(false)
                    .await;

                route_container.load_balancer.update().await.unwrap();

                // TODO: only update if the upstream has changed
                weak_map.insert(route.key().to_string(), route_container);
            }

            // Important: not to hold the lock while updating the route store
            // E.g. inserting while we are cloning items in a loop
            for (key, value) in weak_map.iter() {
                self.route_store.insert(key.clone(), value.clone());
            }

            // We don't need to hold health check results in memory
            drop(weak_map);
        }
    }

    fn name(&self) -> &str {
        "health_check_service"
    }

    fn threads(&self) -> Option<usize> {
        Some(1)
    }
}
