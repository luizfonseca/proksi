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

async fn run_health_check_loop(store: RouteStore, _: ShutdownWatch) {
    let mut interval = tokio::time::interval(Duration::from_secs(15));
    interval.tick().await;

    loop {
        tokio::select! {
            _ = interval.tick() => {
              let mut weak_map = HashMap::new();
              let store_iter = store.iter();

              tracing::debug!("strong count for store is {}", Arc::strong_count(&store));

              for route in store_iter {
                  tracing::debug!("Running health check for host {}", route.key());

                  let route_container = route.clone();
                  route_container
                      .load_balancer
                      .backends()
                      .run_health_check(false)
                      .await;

                  route_container.load_balancer.update().await.unwrap();

                  // TODO: only update if the upstream has changed
                  weak_map.insert(route.key().to_owned(), route_container);
              }

              // Important: not to hold the lock while
              // updating the route store
              // E.g. inserting while we are cloning items in a loop
              for (key, value) in weak_map {
                tracing::debug!("count for {key} is {}", Arc::strong_count(&value));

                  store.insert(key, value);
              }
            }
        }
    }
}

#[async_trait]
impl Service for HealthService {
    async fn start_service(&mut self, _fds: Option<ListenFds>, shutdown: ShutdownWatch) {
        tracing::info!("Starting health check service");

        let handle = tokio::spawn(run_health_check_loop(self.route_store.clone(), shutdown));

        let _ = handle.await;
    }

    fn name(&self) -> &str {
        "health_check_service"
    }

    fn threads(&self) -> Option<usize> {
        Some(1)
    }
}
