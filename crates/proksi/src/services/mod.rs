//! Every service running in the background: what each one is, and when it runs. What they run
//! is in their feature's module (`crate::docker`, `crate::letsencrypt`...).
//!
//! In one service, on one thread (`BackgroundFunctionService`):
//!
//! - Routes, applied as their sources send them: [`routing::RoutingService`]
//! - The configuration files, reloaded when they change: [`config_watch::FileWatcherService`]
//! - Docker labels: [`docker::LabelService`]
//! - Health checks of upstreams: [`health_check::HealthService`]
//! - Let's Encrypt certificates, ordered and renewed: [`letsencrypt::LetsencryptService`]
//! - `notify` webhooks, delivered: [`notify::NotifyService`]
//! - Kubernetes `Ingress`es: `crate::kubernetes::source::run`
//! - The store's routes, certificates and `notify` blocks, synced: [`crate::routing::store_source`]
//! - Metrics: certificates' expiry, and the admin API's live stats: [`crate::metrics`]
//! - The JWKS of `jwt` blocks, refreshed: `proksi_plugins::jwt::jwks`
//! - Connections drained on shutdown: [`crate::proxy_server::connections`]
//!
//! In a service of its own, started in `main`: access and error logs, [`logger::LoggerService`].

use std::{sync::Arc, time::Duration};

use async_trait::async_trait;
use pingora::{
    server::{ListenFds, ShutdownWatch},
    services::{Service, ServiceReadyNotifier},
};
use tokio::sync::mpsc::Receiver;

use crate::{
    config::{Config, StoreType},
    routing::{
        RouteSnapshot, RouteSourceKind, SnapshotSender,
        file_source::file_snapshot,
        store_source::{StoreSource, spawn_isolated},
    },
    stores,
};

use config_watch::FileWatcherService;
use docker::LabelService;
use health_check::HealthService;
use letsencrypt::LetsencryptService;
use notify::NotifyService;
use routing::RoutingService;

pub mod config_watch;
pub mod docker;
pub mod health_check;
pub mod letsencrypt;
pub mod logger;
pub mod notify;
pub mod routing;

/// The background services above, on one thread
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
impl pingora::services::ServiceWithDependents for BackgroundFunctionService {
    /// Ready (and the listening services start) once the routes are applied, see
    /// `routing::readiness`
    async fn start_service(
        &mut self,
        _fds: Option<ListenFds>,
        shutdown: ShutdownWatch,
        listeners: usize,
        ready_notifier: ServiceReadyNotifier,
    ) {
        let Some(route_receiver) = self.route_receiver.take() else {
            tracing::error!("background services already started");
            return;
        };
        let mut routing_service = RoutingService::new(route_receiver);
        let mut notify_service = NotifyService::new(self.config.notify.clone());

        // Routes from the configuration file
        if self
            .route_sender
            .send(file_snapshot(&self.config))
            .await
            .is_err()
        {
            tracing::error!("failed to send the routes from the configuration file");
        }

        let mut health_service = HealthService::new();
        let mut docker_service = LabelService::new(self.config.clone(), self.route_sender.clone());
        let mut letsencrypt_service = LetsencryptService::new(self.config.clone());
        let mut config_server =
            FileWatcherService::new(self.config.clone(), self.route_sender.clone());

        // Routes from the store, shared by every replica (and written by the admin API, which
        // works with the memory store too)
        let shared = !matches!(self.config.store.store_type, StoreType::Memory);
        let store_source = (shared || self.config.admin.is_some()).then(|| {
            StoreSource::new(
                stores::global::get_store().clone(),
                self.route_sender.clone(),
                Duration::from_secs(self.config.store.sync_interval_secs),
            )
        });
        let store_sync = async {
            if let Some(store_source) = store_source {
                if let Err(err) = spawn_isolated(store_source.run()).await {
                    tracing::error!("store sync stopped: {err}");
                }
            }
        };

        // Certificate expiry metrics, from the store
        let certificate_metrics = async {
            if !crate::metrics::enabled() {
                return;
            }
            if let Err(err) = spawn_isolated(crate::metrics::certificate_expiry()).await {
                tracing::error!("certificate metrics stopped: {err}");
            }
        };

        // Live stats for the admin API: the metrics, sampled every second
        let live_stats = async {
            if crate::metrics::enabled() {
                crate::metrics::live::run().await;
            }
        };

        // Hosts from Kubernetes `Ingress`es (ADR 0005)
        let kubernetes = async {
            #[cfg(feature = "kubernetes")]
            if self.config.kubernetes.enabled {
                if matches!(self.config.store.store_type, StoreType::Memory) {
                    tracing::warn!(
                        "the kubernetes ingress controller uses the memory store: with more than one replica, each orders its own certificates (use redis or postgres to share them)"
                    );
                }
                crate::kubernetes::source::run(
                    self.config.kubernetes.clone(),
                    self.route_sender.clone(),
                )
                .await;
            }
        };

        // Keys of the `jwt` blocks' JWKS URLs: waits while no block uses one
        let jwks = crate::blocks::jwt::jwks::refresh();

        let ready = ready(&self.config, shared, ready_notifier);

        let drain = crate::proxy_server::connections::exit_when_drained(
            shutdown.clone(),
            self.config.server.shutdown_timeout(),
        );

        let _ = tokio::join!(
            ready,
            drain,
            store_sync,
            kubernetes,
            certificate_metrics,
            live_stats,
            jwks,
            Service::start_service(&mut routing_service, None, shutdown.clone(), listeners),
            Service::start_service(&mut health_service, None, shutdown.clone(), listeners),
            Service::start_service(&mut config_server, None, shutdown.clone(), listeners),
            Service::start_service(&mut docker_service, None, shutdown.clone(), listeners),
            Service::start_service(&mut letsencrypt_service, None, shutdown.clone(), listeners),
            Service::start_service(&mut notify_service, None, shutdown, listeners),
        );
    }

    fn name(&self) -> &'static str {
        "background_services"
    }

    /// Listens on nothing: inherited sockets no service declares are closed after an upgrade
    fn listen_addresses(&self) -> Option<Vec<String>> {
        Some(vec![])
    }

    fn threads(&self) -> Option<usize> {
        Some(1)
    }
}

/// Reports the background services ready once the routes of every source are applied (and,
/// when upgrading, certificates loaded), so the listening services can start: during an upgrade,
/// the new worker only accepts connections once it serves the hosts the previous one did
async fn ready(config: &Config, store_routes: bool, ready_notifier: ServiceReadyNotifier) {
    let mut sources = vec![RouteSourceKind::File];
    if config.docker.enabled.unwrap_or(false) {
        sources.push(RouteSourceKind::Docker);
    }
    if store_routes || config.admin.is_some() {
        sources.push(RouteSourceKind::Store);
    }
    #[cfg(feature = "kubernetes")]
    if config.kubernetes.enabled {
        sources.push(RouteSourceKind::Kubernetes);
    }
    if !crate::routing::readiness::wait(&sources, config.upgrade).await {
        tracing::warn!(
            "starting to listen before every route or certificate is ready (waited {}s)",
            crate::routing::readiness::READY_TIMEOUT.as_secs()
        );
    }
    ready_notifier.notify_ready();
    #[cfg(target_os = "linux")]
    crate::supervisor::notify_ready();
    crate::notify::emit(|| crate::notify::Event::new(crate::config::notify::Kind::InstanceStarted));
}
