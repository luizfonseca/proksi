use std::{sync::Arc, time::Duration};

use async_trait::async_trait;
use crossbeam_channel::{Receiver, Sender};
use pingora::{
    server::{ListenFds, ShutdownWatch},
    services::Service,
};
use pingora_load_balancing::{health_check::TcpHealthCheck, selection::RoundRobin, LoadBalancer};
use tracing::info;

use crate::{config::Config, MsgProxy, MsgRoute, ROUTE_STORE};

// Service discovery for load balancers
pub struct RoutingService {
    config: Arc<Config>,
    receiver: crossbeam_channel::Receiver<MsgProxy>,
}

impl RoutingService {
    pub fn new(config: Arc<Config>, chan: (Sender<MsgProxy>, Receiver<MsgProxy>)) -> Self {
        Self {
            config,
            receiver: chan.1,
        }
    }

    /// From a given configuration file, create the static load balancing configuration
    fn add_routes_from_config(&mut self) {
        for route in &self.config.routes {
            // For each upstream, create a backend
            let upstream_backends = route
                .upstreams
                .iter()
                .map(|upstr| format!("{}:{}", upstr.ip, upstr.port));

            let mut upstreams =
                LoadBalancer::<RoundRobin>::try_from_iter(upstream_backends).unwrap();
            let tcp_health_check = TcpHealthCheck::new();
            upstreams.set_health_check(tcp_health_check);
            upstreams.health_check_frequency = Some(Duration::from_secs(15));

            ROUTE_STORE.insert(route.host.to_string(), Arc::new(upstreams));

            info!("Added route: {}, {:?}", route.host, route.upstreams);
        }
    }

    fn watch_for_route_changes(&self) -> tokio::task::JoinHandle<()> {
        let receiver = self.receiver.clone();
        let mut interval = tokio::time::interval(Duration::from_secs(1));

        tokio::spawn(async move {
            loop {
                interval.tick().await;
                if let Ok(MsgProxy::NewRoute(route)) = receiver.try_recv() {
                    add_route_from_hook(route);
                }
            }
        })
    }
}

#[async_trait]
impl Service for RoutingService {
    async fn start_service(&mut self, _fds: Option<ListenFds>, _shutdown: ShutdownWatch) {
        // Setup initial routes from config file
        self.add_routes_from_config();

        // Watch for new hosts being added and configure them accordingly
        tokio::select! {
            _ = self.watch_for_route_changes() => {}
        };
    }

    fn name(&self) -> &str {
        "ProxyServiceDiscovery"
    }

    fn threads(&self) -> Option<usize> {
        Some(1)
    }
}

// TODO: find if host already exists but new/old upstreams have changed
fn add_route_from_hook(route: MsgRoute) {
    let mut upstreams = LoadBalancer::<RoundRobin>::try_from_iter(route.upstreams).unwrap();

    let tcp_health_check = TcpHealthCheck::new();
    upstreams.set_health_check(tcp_health_check);
    upstreams.health_check_frequency = Some(Duration::from_secs(15));

    ROUTE_STORE.insert(route.host.to_string(), Arc::new(upstreams));
}
