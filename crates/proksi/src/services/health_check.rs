use async_trait::async_trait;
use pingora::{
    server::{ListenFds, ShutdownWatch},
    services::Service,
};

use crate::health_check;

/// Health check service that will run health checks on all upstreams
/// And update the route store with the new healthy upstreams.
/// This service will run in a separate thread.
pub struct HealthService {}

impl HealthService {
    pub fn new() -> Self {
        Self {}
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

        tokio::join!(
            health_check::run_health_check_loop(),
            health_check::run_resolve_loop()
        );
    }

    fn name(&self) -> &'static str {
        "health_check_service"
    }

    fn threads(&self) -> Option<usize> {
        Some(1)
    }
}
