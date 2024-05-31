use std::{borrow::Cow, fmt::Debug, net::ToSocketAddrs, str::FromStr, sync::Arc, time::Duration};

use async_trait::async_trait;

use http::{HeaderName, HeaderValue};
use pingora::{
    server::{ListenFds, ShutdownWatch},
    services::Service,
};
use pingora_load_balancing::{health_check::TcpHealthCheck, selection::RoundRobin, LoadBalancer};
use tokio::sync::broadcast::Sender;
use tracing::debug;

use crate::{
    config::{Config, RouteHeader, RouteMatcher, RoutePathMatcher},
    stores::routes::{RouteStore, RouteStoreContainer},
    MsgProxy,
};

// Service discovery for load balancers
pub struct RoutingService {
    config: Arc<Config>,
    broadcast: Sender<MsgProxy>,
    store: RouteStore,
}

impl RoutingService {
    pub fn new(config: Arc<Config>, broadcast: Sender<MsgProxy>, store: RouteStore) -> Self {
        Self {
            config,
            broadcast,
            store,
        }
    }

    /// From a given configuration file, create the static load balancing configuration
    fn add_routes_from_config(&mut self) {
        for route in &self.config.routes {
            // For each upstream, create a backend
            let upstream_backends = route
                .upstreams
                .iter()
                .map(|upstr| format!("{}:{}", upstr.ip, upstr.port))
                .collect::<Vec<String>>();

            add_route_to_router(
                &self.store,
                &route.host,
                &upstream_backends,
                route.match_with.clone(),
                route.headers.as_ref(),
            );

            debug!("Added route: {}, {:?}", route.host, route.upstreams);
        }
    }

    /// Watch for new routes being added and update the Router Store
    fn watch_for_route_changes(&self) -> tokio::task::JoinHandle<()> {
        let mut receiver = self.broadcast.subscribe();
        let store = self.store.clone();

        tokio::spawn(async move {
            loop {
                // TODO: refactor
                if let Ok(MsgProxy::NewRoute(route)) = receiver.recv().await {
                    let mut matcher: Option<RouteMatcher> = None;
                    let route_clone = route.path_matchers.clone();
                    if !route.path_matchers.is_empty() {
                        matcher = Some(RouteMatcher {
                            path: Some(RoutePathMatcher {
                                patterns: route_clone
                                    .iter()
                                    .map(|v| Cow::Owned(v.clone()))
                                    .collect(),
                            }),
                        });
                    }

                    add_route_to_router(&store, &route.host, &route.upstreams, matcher, None);
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
        "proxy_service_discovery"
    }

    fn threads(&self) -> Option<usize> {
        Some(1)
    }
}

// TODO: find if host already exists but new/old upstreams have changed
fn add_route_to_router<A, T>(
    store: &RouteStore,
    host: &str,
    upstream_input: &T,
    match_with: Option<RouteMatcher>,
    headers: Option<&RouteHeader>,
) where
    T: IntoIterator<Item = A> + Debug + Clone,
    A: ToSocketAddrs,
{
    let upstreams = LoadBalancer::<RoundRobin>::try_from_iter(upstream_input.clone());
    if upstreams.is_err() {
        debug!(
            "Could not create upstreams for host: {}, upstreams {:?}",
            host, upstream_input
        );
        return;
    }

    let mut upstreams = upstreams.unwrap();

    // TODO: support defining health checks in the configuration file
    let tcp_health_check = TcpHealthCheck::new();
    upstreams.set_health_check(tcp_health_check);
    upstreams.health_check_frequency = Some(Duration::from_secs(15));

    // Create new routing container
    let mut route_store_container = RouteStoreContainer::new(upstreams);

    if let Some(headers) = headers {
        if let Some(headers) = headers.add.as_ref() {
            route_store_container.host_header_add = headers
                .iter()
                .map(|v| {
                    (
                        HeaderName::from_str(&v.name).unwrap(),
                        HeaderValue::from_str(&v.value).unwrap(),
                    )
                })
                .collect();
        }

        if let Some(to_remove) = headers.remove.as_ref() {
            route_store_container.host_header_remove =
                to_remove.iter().map(|v| v.name.to_string()).collect();
        }
    }

    // Prepare route matchers
    // TODO: enable matchers for upstreams for true load balancing based on path
    if let Some(match_with) = match_with {
        // Path matchers
        match match_with.path {
            Some(path_matcher) if !path_matcher.patterns.is_empty() => {
                let pattern = path_matcher.patterns;
                route_store_container.path_matcher.with_pattern(&pattern);
            }
            _ => {}
        }
    }

    store.insert(host.to_string(), Arc::new(route_store_container));
}
