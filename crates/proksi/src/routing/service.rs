use async_trait::async_trait;
use pingora::{
    server::{ListenFds, ShutdownWatch},
    services::Service,
};
use tokio::sync::mpsc::Receiver;

use super::{reconciler::Reconciler, RouteSnapshot, SnapshotSender};

/// Snapshots waiting to be applied. Sources wait when it's full.
const SNAPSHOT_QUEUE_CAPACITY: usize = 64;

/// Channel used by route sources to send their routes to the reconciler
pub fn snapshot_channel() -> (SnapshotSender, Receiver<RouteSnapshot>) {
    tokio::sync::mpsc::channel(SNAPSHOT_QUEUE_CAPACITY)
}

/// Applies the snapshots of every source until all senders are dropped
pub async fn run_reconciler(mut receiver: Receiver<RouteSnapshot>) {
    let mut reconciler = Reconciler::new();
    while let Some(snapshot) = receiver.recv().await {
        reconciler.apply(snapshot).await;
    }
}

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

#[cfg(test)]
mod tests {
    use crate::{
        routing::{test_utils::route, RouteSnapshot, RouteSourceKind},
        stores,
    };

    use super::{run_reconciler, snapshot_channel};

    #[tokio::test]
    async fn test_run_reconciler_applies_snapshots() {
        let (sender, receiver) = snapshot_channel();
        sender
            .send(RouteSnapshot::new(
                RouteSourceKind::File,
                vec![route("service-a.test", "10.0.0.1")],
            ))
            .await
            .unwrap();
        sender
            .send(RouteSnapshot::new(
                RouteSourceKind::Docker,
                vec![route("service-b.test", "10.0.0.2")],
            ))
            .await
            .unwrap();
        drop(sender);

        run_reconciler(receiver).await;

        assert!(stores::get_route_by_key("service-a.test").is_some());
        assert!(stores::get_route_by_key("service-b.test").is_some());
    }
}
