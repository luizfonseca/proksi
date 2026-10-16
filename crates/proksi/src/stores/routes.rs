use std::{
    collections::{BTreeSet, HashMap},
    net::{SocketAddr, ToSocketAddrs},
    sync::Arc,
};

use pingora::lb::{discovery::Static, selection::RoundRobin, Backends, LoadBalancer};

use crate::{
    blocks::Blocks,
    config::{RouteCache, RouteUpstream},
    metrics::LocationMetrics,
};

use super::host_container::HostContainer;

#[derive(Clone)]
pub struct RouteStoreContainer {
    pub load_balancer: Arc<LoadBalancer<RoundRobin>>,

    pub upstreams: Vec<RouteUpstream>,
    /// Resolved address of each upstream (index into `upstreams`)
    upstream_addrs: HashMap<SocketAddr, usize>,

    /// Building blocks (plugins, headers, cache), in the order they run
    pub blocks: Blocks,

    pub cache: Option<RouteCache>,

    /// Requests and upstreams of this location (ADR 0004)
    pub metrics: Option<LocationMetrics>,
}

impl Default for RouteStoreContainer {
    fn default() -> Self {
        RouteStoreContainer {
            load_balancer: Arc::new(LoadBalancer::<RoundRobin>::from_backends(Backends::new(
                Static::new(BTreeSet::new()),
            ))),
            blocks: Blocks::default(),
            upstreams: Vec::new(),
            upstream_addrs: HashMap::new(),
            cache: None,
            metrics: None,
        }
    }
}

impl RouteStoreContainer {
    pub fn new(load_balancer: LoadBalancer<RoundRobin>) -> Self {
        RouteStoreContainer {
            load_balancer: Arc::new(load_balancer),
            blocks: Blocks::default(),
            upstreams: Vec::with_capacity(5),
            upstream_addrs: HashMap::new(),
            cache: None,
            metrics: None,
        }
    }

    /// Sets the upstreams of this route, resolving their addresses once so that
    /// requests don't need to resolve them again (the load balancer backends are
    /// resolved at the same time).
    pub fn set_upstreams(&mut self, upstreams: Vec<RouteUpstream>) {
        let mut upstream_addrs = HashMap::new();
        for (index, upstream) in upstreams.iter().enumerate() {
            let Ok(addrs) = format!("{}:{}", upstream.ip, upstream.port).to_socket_addrs() else {
                tracing::warn!(
                    "could not resolve upstream {}:{}",
                    upstream.ip,
                    upstream.port
                );
                continue;
            };

            for addr in addrs {
                upstream_addrs.entry(addr).or_insert(index);
            }
        }

        self.upstream_addrs = upstream_addrs;
        self.upstreams = upstreams;
    }

    /// The index in `upstreams` of a resolved (load balancer) address
    pub fn upstream_index(&self, addr: &SocketAddr) -> Option<usize> {
        self.upstream_addrs.get(addr).copied()
    }
}

/// The hosts being served, by lowercase name (updated by the reconciler)
pub type RouteStore = papaya::HashMap<String, Arc<HostContainer>>;

#[cfg(test)]
mod tests {
    use std::borrow::Cow;

    use super::*;

    #[test]
    fn test_router_container_finds_upstream_by_resolved_addr() {
        let load_balancer = LoadBalancer::<RoundRobin>::try_from_iter(vec!["1.1.1.1:80"]).unwrap();
        let mut route_store = RouteStoreContainer::new(load_balancer);
        route_store.set_upstreams(vec![
            RouteUpstream {
                ip: Cow::Borrowed("127.0.0.1"),
                port: 8080,
                sni: Some("ip.test".to_string()),
                ..RouteUpstream::default()
            },
            RouteUpstream {
                ip: Cow::Borrowed("localhost"),
                port: 9090,
                sni: Some("hostname.test".to_string()),
                ..RouteUpstream::default()
            },
            RouteUpstream {
                ip: Cow::Borrowed("does-not-resolve.invalid"),
                port: 80,
                ..RouteUpstream::default()
            },
        ]);

        let upstream = |addr: &str| {
            route_store
                .upstream_index(&addr.parse().unwrap())
                .and_then(|index| route_store.upstreams.get(index))
        };
        assert_eq!(upstream("127.0.0.1:8080").unwrap().sni.as_deref(), Some("ip.test"));
        assert_eq!(
            upstream("127.0.0.1:9090").unwrap().sni.as_deref(),
            Some("hostname.test")
        );
        assert!(upstream("127.0.0.1:1234").is_none());
        assert_eq!(route_store.upstreams.len(), 3);
    }
}
