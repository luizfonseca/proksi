use async_trait::async_trait;
use notify::{EventHandler, Watcher};
use pingora::{
    server::{ListenFds, ShutdownWatch},
    services::Service,
};

pub struct ConfigService {}

impl ConfigService {
    pub fn new() -> Self {
        Self {}
    }
}

pub struct ConfigServiceHandler {}
impl EventHandler for ConfigServiceHandler {
    fn handle_event(&mut self, event: notify::Result<notify::Event>) {
        println!("ConfigService: {:?}", event);
    }
}

#[async_trait]
impl Service for ConfigService {
    async fn start_service(&mut self, _fds: Option<ListenFds>, _shutdown: ShutdownWatch) {
        tracing::info!("starting config watcher service");

        let mut watcher = notify::poll::PollWatcher::new(
            ConfigServiceHandler {},
            notify::Config::default().with_manual_polling(),
        )
        .unwrap();

        watcher
            .watch(
                std::path::Path::new("./dist"),
                notify::RecursiveMode::Recursive,
            )
            .unwrap();

        let mut interval = tokio::time::interval(std::time::Duration::from_secs(10));
        interval.tick().await;

        loop {
            interval.tick().await;
            if let Ok(_) = watcher.poll() {
                tracing::info!("config watcher service tick");
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
