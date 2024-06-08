use std::{borrow::Cow, str::FromStr, sync::Arc, time::Duration};

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
    config::{Config, RouteHeader, RouteMatcher, RoutePathMatcher, RoutePlugin},
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

            let self_signed_cert_on_failure = route
                .ssl_certificate
                .as_ref()
                .and_then(|v| v.self_signed_on_failure);

            add_route_to_router(
                &self.store,
                &route.host,
                &upstream_backends,
                route.match_with.clone(),
                route.headers.as_ref(),
                route.plugins.as_ref(),
                self_signed_cert_on_failure.unwrap_or(false),
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

                    let route_header = RouteHeader {
                        add: Some(route.host_headers_add),
                        remove: Some(route.host_headers_remove),
                    };

                    add_route_to_router(
                        &store,
                        &route.host,
                        &route.upstreams,
                        matcher,
                        Some(&route_header),
                        Some(&route.plugins),
                        route.self_signed_certs,
                    );

                    tracing::debug!(
                        "Added route: {}, {:?} self-signed: {}",
                        route.host,
                        route.upstreams,
                        route.self_signed_certs
                    );
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

// Check whether the host already exists and whether there the upstream list has changed
fn has_new_backend(store: &RouteStore, host: &str, upstream_input: &[String]) -> bool {
    if let Some(route_container) = store.get(host) {
        let backends = route_container.load_balancer.backends().get_backend();

        // If upstreams are not the same length, return true (update)
        if backends.len() != upstream_input.len() {
            return true;
        }

        !backends
            .iter()
            .all(|be| upstream_input.contains(&be.addr.to_string()))
    } else {
        false
    }
}

// TODO: find if host already exists but new/old upstreams have changed
fn add_route_to_router(
    store: &RouteStore,
    host: &str,
    upstream_input: &[String],
    match_with: Option<RouteMatcher>,
    headers: Option<&RouteHeader>,
    plugins: Option<&Vec<RoutePlugin>>,
    should_self_sign_cert_on_failure: bool,
) {
    // Check if current route already exists
    if store.contains_key(host) && !has_new_backend(store, host, upstream_input) {
        tracing::debug!("skipping update, no routing changes for host: {}", host);
        return;
    }

    let upstreams = LoadBalancer::<RoundRobin>::try_from_iter(upstream_input);
    if upstreams.is_err() {
        tracing::info!(
            "Could not create upstreams for host: {}, upstreams {:?}",
            host,
            upstream_input
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
    route_store_container.self_signed_certificate = should_self_sign_cert_on_failure;

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

    if let Some(plugins) = plugins {
        for plugin in plugins {
            match plugin.name.as_ref() {
                "oauth2" => {
                    route_store_container
                        .plugins
                        .insert(plugin.name.to_string(), plugin.clone());
                }
                "request_id" => continue,
                _ => {}
            }
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stores::routes::RouteStore;
    use dashmap::DashMap;

    fn setup_mock_route_store() -> RouteStore {
        Arc::new(DashMap::new())
    }

    fn setup_route_store_with_entry() -> RouteStore {
        let store = setup_mock_route_store();
        let upstreams = vec!["127.0.0.1:8080".to_string(), "127.0.0.2:8080".to_string()];

        let load_balancer =
            LoadBalancer::<RoundRobin>::try_from_iter(upstreams.into_iter()).unwrap();
        store.insert(
            "example.com".to_string(),
            Arc::new(RouteStoreContainer::new(load_balancer)),
        );

        store
    }

    #[test]
    fn test_add_route_to_router_new_route() {
        let store = setup_mock_route_store();
        let host = "example.com";
        let upstreams = vec!["127.0.0.1:8080".to_string()];
        let matcher = None;
        let headers = None;
        let plugins = None;
        let should_self_sign_cert_on_failure = false;

        add_route_to_router(
            &store,
            host,
            &upstreams,
            matcher,
            headers,
            plugins,
            should_self_sign_cert_on_failure,
        );

        assert!(store.contains_key(host));
    }

    #[test]
    fn test_add_route_to_router_existing_route_no_changes() {
        let store = setup_route_store_with_entry();
        let host = "example.com";
        let upstreams = vec!["127.0.0.1:8080".to_string()];
        let matcher = None;
        let headers = None;
        let plugins = None;
        let should_self_sign_cert_on_failure = false;

        add_route_to_router(
            &store,
            host,
            &upstreams,
            matcher,
            headers,
            plugins,
            should_self_sign_cert_on_failure,
        );

        // Verify the route still exists and no new upstreams were added
        assert!(store.contains_key(host));
    }

    #[test]
    fn test_has_new_backend_no_change() {
        let store = setup_route_store_with_entry();
        let host = "example.com";
        let upstreams = vec!["127.0.0.1:8080".to_string(), "127.0.0.2:8080".to_string()];

        assert!(!has_new_backend(&store, host, &upstreams));
    }

    #[test]
    fn test_has_new_backend_with_change() {
        let store = setup_route_store_with_entry();
        let host = "example.com";
        let upstreams = vec!["127.0.0.3:8080".to_string()];

        assert!(has_new_backend(&store, host, &upstreams));
    }

    #[test]
    fn test_add_route_to_router_existing_route_with_changes() {
        let store = setup_route_store_with_entry();
        let host = "example.com";
        let upstreams = vec!["127.0.0.3:8080".to_string()];
        let matcher = None;
        let headers = None;
        let plugins = None;
        let should_self_sign_cert_on_failure = false;

        add_route_to_router(
            &store,
            host,
            &upstreams,
            matcher,
            headers,
            plugins,
            should_self_sign_cert_on_failure,
        );

        // Verify that the route exists and the upstreams have been updated
        assert!(store.contains_key(host));
        let route_container = store.get(host).unwrap();
        let backends: Vec<String> = route_container
            .load_balancer
            .backends()
            .get_backend()
            .iter()
            .map(|backend| backend.addr.to_string())
            .collect();
        assert!(backends.contains(&"127.0.0.3:8080".to_string()));
    }
}
