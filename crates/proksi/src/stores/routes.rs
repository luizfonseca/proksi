use std::{collections::BTreeSet, sync::Arc};

use pingora::lb::{discovery::Static, selection::RoundRobin, Backend, Backends, LoadBalancer};

use crate::{
    blocks::Blocks,
    config::{
        host::{CompiledRewrite, LocationTimeouts},
        RouteCache, RouteUpstream,
    },
    metrics::LocationMetrics,
    routing::build::upstream_index,
};

use super::host_container::HostContainer;

#[derive(Clone)]
pub struct RouteStoreContainer {
    /// The location's path pattern
    pub path: String,
    pub load_balancer: Arc<LoadBalancer<RoundRobin>>,

    pub upstreams: Vec<RouteUpstream>,

    /// Building blocks (plugins, headers, cache), in the order they run
    pub blocks: Blocks,

    pub cache: Option<RouteCache>,

    /// Timeouts of the location's requests, instead of `server.timeouts`
    pub timeouts: Option<LocationTimeouts>,

    /// How the path sent to upstreams is changed
    pub rewrite: Option<CompiledRewrite>,

    /// Requests and upstreams of this location (ADR 0004)
    pub metrics: Option<LocationMetrics>,
    /// Rules of the location's requests (ADR 0007)
    pub rules: Option<Arc<crate::rules::Rules>>,
    /// Shares of the clients sent to other upstreams, numbered after the location's own in
    /// `upstreams`
    pub splits: Arc<Vec<crate::routing::split::Split>>,
}

impl Default for RouteStoreContainer {
    fn default() -> Self {
        RouteStoreContainer {
            path: String::new(),
            load_balancer: Arc::new(LoadBalancer::<RoundRobin>::from_backends(Backends::new(
                Static::new(BTreeSet::new()),
            ))),
            blocks: Blocks::default(),
            upstreams: Vec::new(),
            cache: None,
            timeouts: None,
            rewrite: None,
            metrics: None,
            rules: None,
            splits: Arc::default(),
        }
    }
}

impl RouteStoreContainer {
    pub fn new(load_balancer: LoadBalancer<RoundRobin>) -> Self {
        RouteStoreContainer {
            path: String::new(),
            load_balancer: Arc::new(load_balancer),
            blocks: Blocks::default(),
            upstreams: Vec::with_capacity(5),
            cache: None,
            timeouts: None,
            rewrite: None,
            metrics: None,
            rules: None,
            splits: Arc::default(),
        }
    }

    /// Sets the upstreams of this route (the ones its load balancer was built with)
    pub fn set_upstreams(&mut self, upstreams: Vec<RouteUpstream>) {
        self.upstreams = upstreams;
    }

    /// The load balancers of the location: its own, then its traffic splits'
    pub fn load_balancers(&self) -> impl Iterator<Item = &Arc<LoadBalancer<RoundRobin>>> {
        std::iter::once(&self.load_balancer)
            .chain(self.splits.iter().map(|split| &split.load_balancer))
    }

    /// Whether each upstream is healthy, by the health checks: when one of its addresses is
    pub fn upstream_health(&self) -> Vec<bool> {
        let mut healthy = vec![false; self.upstreams.len()];
        for load_balancer in self.load_balancers() {
            let backends = load_balancer.backends();
            for backend in backends.get_backend().iter() {
                if let Some(slot) = upstream_index(backend).and_then(|index| healthy.get_mut(index))
                {
                    *slot |= backends.ready(backend);
                }
            }
        }
        healthy
    }

    /// The addresses each upstream resolved to, and whether each is healthy
    pub fn upstream_addresses(&self) -> Vec<Vec<(String, bool)>> {
        let mut addresses = vec![vec![]; self.upstreams.len()];
        for load_balancer in self.load_balancers() {
            let found =
                crate::routing::build::upstream_addresses(load_balancer, self.upstreams.len());
            for (all, found) in addresses.iter_mut().zip(found) {
                all.extend(found);
            }
        }
        for list in &mut addresses {
            list.sort();
        }
        addresses
    }

    /// The traffic split holding the upstream at `index`, if any
    pub fn split_of(&self, index: usize) -> Option<&crate::routing::split::Split> {
        self.splits.iter().find(|split| split.holds(index))
    }

    /// The upstream a backend of the load balancer came from
    pub fn upstream(&self, backend: &Backend) -> Option<(usize, &RouteUpstream)> {
        let index = upstream_index(backend)?;
        Some((index, self.upstreams.get(index)?))
    }
}

/// The hosts being served, by lowercase name (updated by the reconciler)
pub type RouteStore = papaya::HashMap<String, Arc<HostContainer>>;

#[cfg(test)]
mod tests {
    use std::borrow::Cow;

    use super::*;

    #[test]
    fn test_router_container_finds_the_upstream_of_a_backend() {
        let upstreams = vec![
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
        ];
        let mut route_store =
            RouteStoreContainer::new(crate::routing::build::load_balancer(&upstreams));
        route_store.set_upstreams(upstreams);

        let backends = route_store.load_balancer.backends().get_backend();
        let sni = |addr: &str| {
            let backend = backends.iter().find(|b| b.addr.to_string() == addr)?;
            route_store.upstream(backend)?.1.sni.clone()
        };
        assert_eq!(sni("127.0.0.1:8080").as_deref(), Some("ip.test"));
        assert_eq!(sni("127.0.0.1:9090").as_deref(), Some("hostname.test"));
        assert!(sni("127.0.0.1:1234").is_none());
    }
}
