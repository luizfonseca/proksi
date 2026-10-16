use std::{
    collections::{BTreeSet, HashMap},
    net::{SocketAddr, ToSocketAddrs},
    sync::Arc,
};

use http::{HeaderName, HeaderValue};
use pingora::lb::{discovery::Static, selection::RoundRobin, Backends, LoadBalancer};

use crate::config::{RouteCache, RoutePlugin, RouteUpstream};

use super::host_container::HostContainer;

#[derive(Clone)]
pub struct RouteStoreContainer {
    pub load_balancer: Arc<LoadBalancer<RoundRobin>>,
    pub host_header_remove: Vec<String>,
    pub host_header_add: Vec<(HeaderName, HeaderValue)>,

    pub upstreams: Vec<RouteUpstream>,
    /// Resolved address of each upstream (index into `upstreams`)
    upstream_addrs: HashMap<SocketAddr, usize>,

    pub plugins: HashMap<String, RoutePlugin>,

    pub cache: Option<RouteCache>,
}

impl Default for RouteStoreContainer {
    fn default() -> Self {
        RouteStoreContainer {
            load_balancer: Arc::new(LoadBalancer::<RoundRobin>::from_backends(Backends::new(
                Static::new(BTreeSet::new()),
            ))),
            host_header_remove: Vec::new(),
            host_header_add: Vec::new(),
            plugins: HashMap::new(),
            upstreams: Vec::new(),
            upstream_addrs: HashMap::new(),
            cache: None,
        }
    }
}

impl RouteStoreContainer {
    pub fn new(load_balancer: LoadBalancer<RoundRobin>) -> Self {
        RouteStoreContainer {
            load_balancer: Arc::new(load_balancer),
            host_header_remove: Vec::with_capacity(5),
            host_header_add: Vec::with_capacity(5),
            plugins: HashMap::new(),
            upstreams: Vec::with_capacity(5),
            upstream_addrs: HashMap::new(),
            cache: None,
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

    /// Finds the upstream configuration for a resolved (load balancer) address
    pub fn upstream_for(&self, addr: &SocketAddr) -> Option<&RouteUpstream> {
        self.upstream_addrs
            .get(addr)
            .and_then(|index| self.upstreams.get(*index))
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

        let by_ip = route_store.upstream_for(&"127.0.0.1:8080".parse().unwrap());
        assert_eq!(by_ip.unwrap().sni.as_deref(), Some("ip.test"));

        let by_hostname = route_store.upstream_for(&"127.0.0.1:9090".parse().unwrap());
        assert_eq!(by_hostname.unwrap().sni.as_deref(), Some("hostname.test"));

        assert!(route_store
            .upstream_for(&"127.0.0.1:1234".parse().unwrap())
            .is_none());
        assert_eq!(route_store.upstreams.len(), 3);
    }
}
