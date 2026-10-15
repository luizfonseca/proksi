use std::{sync::Arc, time::Duration};

use async_trait::async_trait;
use config::FileWatcherService;
use docker::LabelService;
use letsencrypt::http01::LetsencryptService;
use pingora::server::{ListenFds, ShutdownWatch};
use tokio::sync::mpsc::Receiver;

use crate::{
    config::{Config, StoreType},
    routing::{
        file_source::file_snapshot, service::RoutingService, store_source::StoreSource,
        RouteSnapshot, SnapshotSender,
    },
    stores,
};

pub mod config;
pub mod docker;
pub mod health_check;
pub mod letsencrypt;
pub mod logger;

/// Exploring: what if we grouped all the services into a single service using a single thread?
pub struct BackgroundFunctionService {
    config: Arc<Config>,
    route_sender: SnapshotSender,
    route_receiver: Option<Receiver<RouteSnapshot>>,
}

impl BackgroundFunctionService {
    pub fn new(
        config: Arc<Config>,
        route_sender: SnapshotSender,
        route_receiver: Receiver<RouteSnapshot>,
    ) -> Self {
        Self {
            config,
            route_sender,
            route_receiver: Some(route_receiver),
        }
    }
}

#[async_trait]
impl pingora::services::Service for BackgroundFunctionService {
    async fn start_service(
        &mut self,
        _fds: Option<ListenFds>,
        shutdown: ShutdownWatch,
        _listeners_per_fd: usize,
    ) {
        let Some(route_receiver) = self.route_receiver.take() else {
            tracing::error!("background services already started");
            return;
        };
        let mut routing_service = RoutingService::new(route_receiver);

        // Routes from the configuration file
        if self
            .route_sender
            .send(file_snapshot(&self.config))
            .await
            .is_err()
        {
            tracing::error!("failed to send the routes from the configuration file");
        }

        let mut health_service = health_check::HealthService::new();
        let mut docker_service = LabelService::new(self.config.clone(), self.route_sender.clone());
        let mut letsencrypt_service = LetsencryptService::new(self.config.clone());
        let mut config_server =
            FileWatcherService::new(self.config.clone(), self.route_sender.clone());

        // Routes from the store, shared by every replica
        let store_source =
            (!matches!(self.config.store.store_type, StoreType::Memory)).then(|| {
                StoreSource::new(
                    stores::global::get_store().clone(),
                    self.route_sender.clone(),
                    Duration::from_secs(self.config.store.sync_interval_secs),
                )
            });
        let store_sync = async {
            if let Some(store_source) = store_source {
                store_source.run().await;
            }
        };

        let _ = tokio::join!(
            store_sync,
            routing_service.start_service(None, shutdown.clone(), _listeners_per_fd),
            health_service.start_service(None, shutdown.clone(), _listeners_per_fd),
            config_server.start_service(None, shutdown.clone(), _listeners_per_fd),
            docker_service.start_service(None, shutdown.clone(), _listeners_per_fd),
            letsencrypt_service.start_service(None, shutdown, _listeners_per_fd),
        );
    }

    fn name(&self) -> &'static str {
        "background_services"
    }

    fn threads(&self) -> Option<usize> {
        Some(1)
    }
}
