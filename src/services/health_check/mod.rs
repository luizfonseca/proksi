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

        loop {
            interval.tick().await;

            for route in ROUTE_STORE.iter_mut() {
                info!("Running health check");
                let upstream = route.value();
                upstream.backends().run_health_check(true).await;
                upstream.update().await.ok();
                // upstream.backends().run_health_check(true).await;
                // upstream.update().await.ok();
            }

            // upstream.backends().run_health_check(true).await;
            // upstream.update().await.ok();
            // info!("Running health check");
        }
    }

    fn name(&self) -> &str {
        "healthservice"
    }

    fn threads(&self) -> Option<usize> {
        Some(1)
    }
}
