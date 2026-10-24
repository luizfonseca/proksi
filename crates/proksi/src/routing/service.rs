use std::time::Duration;

use tokio::sync::mpsc::Receiver;

use super::{reconciler::Reconciler, RouteSnapshot, SnapshotSender};

/// Snapshots waiting to be applied. Sources wait when it's full.
const SNAPSHOT_QUEUE_CAPACITY: usize = 64;

/// Channel used by route sources to send their routes to the reconciler
pub fn snapshot_channel() -> (SnapshotSender, Receiver<RouteSnapshot>) {
    tokio::sync::mpsc::channel(SNAPSHOT_QUEUE_CAPACITY)
}

/// How often certificate files are checked for changes
const CERTIFICATE_FILES_INTERVAL: Duration = Duration::from_secs(10);

/// Applies the snapshots of every source until all senders are dropped, and reloads
/// certificate files when they change
pub async fn run_reconciler(mut receiver: Receiver<RouteSnapshot>) {
    let mut reconciler = Reconciler::new();
    let mut certificate_files = tokio::time::interval(CERTIFICATE_FILES_INTERVAL);
    certificate_files.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    loop {
        tokio::select! {
            snapshot = receiver.recv() => match snapshot {
                Some(snapshot) => {
                    reconciler.apply(snapshot).await;
                }
                None => break,
            },
            _ = certificate_files.tick() => {
                reconciler.reload_certificate_files().await;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        routing::{test_utils::host, RouteSnapshot, RouteSourceKind},
        stores,
    };

    use super::{run_reconciler, snapshot_channel};

    #[tokio::test]
    async fn test_run_reconciler_applies_snapshots() {
        let (sender, receiver) = snapshot_channel();
        sender
            .send(RouteSnapshot::new(
                RouteSourceKind::File,
                vec![host("service-a.test", "10.0.0.1")],
            ))
            .await
            .unwrap();
        sender
            .send(RouteSnapshot::new(
                RouteSourceKind::Docker,
                vec![host("service-b.test", "10.0.0.2")],
            ))
            .await
            .unwrap();
        drop(sender);

        run_reconciler(receiver).await;

        assert!(stores::get_route_by_key("service-a.test").is_some());
        assert!(stores::get_route_by_key("service-b.test").is_some());
    }
}
