use std::sync::Arc;

use async_trait::async_trait;
use config::FileWatcherService;
use discovery::RoutingService;
use docker::LabelService;
use letsencrypt::http01::LetsencryptService;
use pingora::server::{ListenFds, ShutdownWatch};
use tokio::sync::broadcast::Sender;

use crate::{config::Config, MsgProxy};

pub mod config;
pub mod discovery;
pub mod docker;
pub mod health_check;
pub mod letsencrypt;
pub mod logger;

/// Exploring: what if we grouped all the services into a single service using a single thread?
pub struct BackgroundFunctionService {
    config: Arc<Config>,
    broadcast: Sender<MsgProxy>,
}

impl BackgroundFunctionService {
    pub fn new(config: Arc<Config>, broadcast: Sender<MsgProxy>) -> Self {
        Self { config, broadcast }
    }
}

#[async_trait]
impl pingora::services::Service for BackgroundFunctionService {
    async fn start_service(
        &mut self,
        _fds: Option<ListenFds>,
        shutdown: ShutdownWatch,
        _graceful_shutdown_timeout: usize,
    ) {
        let mut routing_service = RoutingService::new(self.config.clone(), self.broadcast.clone());

        let mut health_service = health_check::HealthService::new();
        let mut docker_service = LabelService::new(self.config.clone(), self.broadcast.clone());
        let mut letsencrypt_service = LetsencryptService::new(self.config.clone());
        let mut config_server = FileWatcherService::new(self.config.clone());

        let _ = tokio::join!(
            routing_service.start_service(None, shutdown.clone(), _graceful_shutdown_timeout),
            health_service.start_service(None, shutdown.clone(), _graceful_shutdown_timeout),
            config_server.start_service(None, shutdown.clone(), _graceful_shutdown_timeout),
            docker_service.start_service(None, shutdown.clone(), _graceful_shutdown_timeout),
            letsencrypt_service.start_service(None, shutdown, _graceful_shutdown_timeout),
        );
    }

    fn name(&self) -> &'static str {
        "background_services"
    }

    fn threads(&self) -> Option<usize> {
        Some(1)
    }
}
