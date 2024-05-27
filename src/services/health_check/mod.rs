use std::time::Duration;

use async_trait::async_trait;
use pingora::{
    server::{ListenFds, ShutdownWatch},
    services::Service,
};
use tracing::info;

use crate::ROUTE_STORE;

pub struct HealthService {}

#[async_trait]
impl Service for HealthService {
    async fn start_service(&mut self, _fds: Option<ListenFds>, _shutdown: ShutdownWatch) {
        // TODO: create multiple interval checks
        let mut interval = tokio::time::interval(Duration::from_secs(15));
        interval.tick().await;

        loop {
            interval.tick().await;

            let store_clone = ROUTE_STORE.clone();

            for route in store_clone.iter() {
                info!("Running health check on {}", route.key());
                let upstream = route.value();
                upstream.backends().run_health_check(false).await;
                upstream.update().await.unwrap();
                ROUTE_STORE.insert(route.key().to_string(), upstream.clone());
            }
        }
    }

    fn name(&self) -> &str {
        "healthservice"
    }

    fn threads(&self) -> Option<usize> {
        Some(1)
    }
}
