use std::{collections::HashMap, sync::Arc, time::Duration};

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

async fn run_health_check_loop(store: RouteStore) {
    let mut interval = tokio::time::interval(Duration::from_secs(20));
    interval.tick().await;

    loop {
        interval.tick().await;
        let mut weak_map = HashMap::new();

        for route in store.iter() {
            tracing::debug!("Running health check for host {}", route.key());

            // let route_container = route.to_owned();

            route.load_balancer.update().await.ok();
            route.load_balancer.backends().run_health_check(true).await;

            // TODO: only update if the upstream has changed
            weak_map.insert(route.key().to_owned(), route.clone());
        }

        // Important: not to hold the lock while
        // updating the route store
        // E.g. inserting while we are cloning items in a loop
        for (key, value) in weak_map {
            store.insert(key, value);
        }
    }
}

#[async_trait]
impl Service for HealthService {
    async fn start_service(&mut self, _fds: Option<ListenFds>, _shutdown: ShutdownWatch) {
        tracing::info!("Starting health check service");

        run_health_check_loop(Arc::clone(&self.route_store)).await;
    }

    fn name(&self) -> &str {
        "health_check_service"
    }

    fn threads(&self) -> Option<usize> {
        Some(1)
    }
}
