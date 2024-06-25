use std::sync::Arc;

use async_trait::async_trait;
use notify::{EventHandler, Watcher};
use pingora::{
    server::{ListenFds, ShutdownWatch},
    services::Service,
};

use crate::config::Config;

pub struct FileWatcherService {
    config: Arc<Config>,
}

impl FileWatcherService {
    pub fn new(config: Arc<Config>) -> Self {
        Self { config }
    }
}

pub struct FileWatcherServiceHandler {}
impl EventHandler for FileWatcherServiceHandler {
    fn handle_event(&mut self, event: notify::Result<notify::Event>) {
        println!("ConfigService: {event:?}");
    }
}

#[async_trait]
impl Service for FileWatcherService {
    async fn start_service(&mut self, _fds: Option<ListenFds>, _shutdown: ShutdownWatch) {
        if self.config.auto_reload.enabled.is_some_and(|v| !v) {
            // Nothing to do, lets encrypt is disabled
            return;
        }

        tracing::info!("starting config watcher service");

        let mut watcher = notify::poll::PollWatcher::new(
            FileWatcherServiceHandler {},
            notify::Config::default().with_manual_polling(),
        )
        .unwrap();

        watcher
            .watch(
                std::path::Path::new("./dist"),
                notify::RecursiveMode::Recursive,
            )
            .unwrap();

        let mut interval = tokio::time::interval(std::time::Duration::from_secs(30));
        interval.tick().await;

        loop {
            interval.tick().await;
            if watcher.poll().is_ok() {
                tracing::debug!("config watcher service tick");
            }
        }
    }

    fn name(&self) -> &str {
        "config_watcher_service"
    }

    fn threads(&self) -> Option<usize> {
        Some(1)
    }
}
