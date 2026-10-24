use async_trait::async_trait;
use pingora::{
    server::{ListenFds, ShutdownWatch},
    services::Service,
};
use tokio::sync::mpsc::Receiver;

use crate::routing::{RouteSnapshot, service::run_reconciler};

/// Service running the reconciler
pub struct RoutingService {
    receiver: Option<Receiver<RouteSnapshot>>,
}

impl RoutingService {
    pub fn new(receiver: Receiver<RouteSnapshot>) -> Self {
        Self {
            receiver: Some(receiver),
        }
    }
}

#[async_trait]
impl Service for RoutingService {
    async fn start_service(
        &mut self,
        _fds: Option<ListenFds>,
        _shutdown: ShutdownWatch,
        _listeners_per_fd: usize,
    ) {
        if let Some(receiver) = self.receiver.take() {
            run_reconciler(receiver).await;
        }
    }

    fn name(&self) -> &'static str {
        "routing_service"
    }

    fn threads(&self) -> Option<usize> {
        Some(1)
    }
}
