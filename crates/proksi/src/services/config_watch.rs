use std::sync::Arc;

use async_trait::async_trait;
use pingora::{
    server::{ListenFds, ShutdownWatch},
    services::Service,
};

use crate::{
    config::{watch, Config},
    routing::SnapshotSender,
};

/// Polls the configuration files, and applies their changes (`auto_reload`)
pub struct FileWatcherService {
    config: Arc<Config>,
    sender: SnapshotSender,
}

impl FileWatcherService {
    pub fn new(config: Arc<Config>, sender: SnapshotSender) -> Self {
        Self { config, sender }
    }
}

#[async_trait]
impl Service for FileWatcherService {
    async fn start_service(
        &mut self,
        _fds: Option<ListenFds>,
        _shutdown: ShutdownWatch,
        _listeners_per_fd: usize,
    ) {
        if self.config.auto_reload.enabled.is_some_and(|v| !v) {
            // Nothing to do, auto reload is  disabled
            return;
        }

        // watch main config file:
        let Some(config_path) = &self.config.config_path else {
            // No config path provided, nothing to watch
            tracing::info!("No config path provided, config watcher service not started");
            return;
        };
        tracing::info!("starting config watcher service");

        let Some(watcher) = watch::watcher(&self.config, config_path, self.sender.clone()) else {
            return;
        };

        let mut interval = tokio::time::interval(std::time::Duration::from_secs(
            self.config.auto_reload.interval_secs.unwrap_or(60),
        ));
        interval.tick().await;

        loop {
            interval.tick().await;
            if watcher.poll().is_ok() {
                tracing::debug!("config watcher service tick");
            }
        }
    }

    fn name(&self) -> &'static str {
        "config_watcher_service"
    }

    fn threads(&self) -> Option<usize> {
        Some(1)
    }
}
