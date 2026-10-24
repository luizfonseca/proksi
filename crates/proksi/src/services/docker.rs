use std::{collections::HashMap, sync::Arc, time::Duration};

use async_trait::async_trait;
use bollard::Docker;
use pingora::{
    server::{ListenFds, ShutdownWatch},
    services::Service,
};
use tracing::info;

use crate::{
    config::Config,
    docker::{self, routes::DockerRoutes},
    routing::{RouteSnapshot, RouteSourceKind, SnapshotSender},
};

/// A service that will list all services in a Swarm OR containers through the Docker API
/// and update the route store with the new services.
/// This service will run in a separate thread.
pub struct LabelService {
    config: Arc<Config>,
    inner: Option<Docker>,
    sender: SnapshotSender,
    routes: DockerRoutes,
}

impl LabelService {
    pub fn new(config: Arc<Config>, sender: SnapshotSender) -> Self {
        Self {
            inner: docker::connect(&config),
            config,
            sender,
            routes: DockerRoutes::default(),
        }
    }
}

#[async_trait]
impl Service for LabelService {
    async fn start_service(
        &mut self,
        _fds: Option<ListenFds>,
        mut _shutdown: ShutdownWatch,
        _listeners_per_fd: usize,
    ) {
        if self.config.docker.enabled.is_some_and(|v| !v) {
            // Nothing to do, docker is disabled
            return;
        }

        info!(service = "docker", "Started Docker service");

        let mut interval = tokio::time::interval(Duration::from_secs(
            self.config.docker.interval_secs.unwrap_or(15),
        ));

        interval.tick().await;
        loop {
            interval.tick().await;
            let found = match &self.inner {
                Some(client) => docker::routes(client, &self.config.docker.mode).await,
                None => HashMap::default(),
            };
            let routes = self.routes.update(found);
            let snapshot = RouteSnapshot::new(RouteSourceKind::Docker, routes);
            if self.sender.send(snapshot).await.is_err() {
                tracing::error!("failed to send the routes found in docker");
            }
        }
    }

    fn name(&self) -> &'static str {
        "docker_service"
    }

    fn threads(&self) -> Option<usize> {
        Some(1)
    }
}
