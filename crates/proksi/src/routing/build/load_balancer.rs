//! Load balancers between upstreams, shared by locations and `tcp` blocks. Upstream names are
//! resolved again on a schedule and after failed connections (ADR 0006).

use std::{
    collections::BTreeSet,
    future::Future,
    net::{IpAddr, SocketAddr, ToSocketAddrs},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex, PoisonError,
    },
    task::{Context, Waker},
    time::{Duration, Instant},
};

use async_trait::async_trait;
use http::Extensions;
use pingora::{
    lb::{discovery::ServiceDiscovery, selection::RoundRobin, Backend, Backends, LoadBalancer},
    protocols::l4::socket::SocketAddr as BackendAddr,
};

use crate::config::RouteUpstream;

/// Names are never resolved more often than this, however many connections fail
pub const MIN_RESOLVE_INTERVAL: Duration = Duration::from_secs(1);

/// How long a lookup can take before its last addresses are kept
const LOOKUP_TIMEOUT: Duration = Duration::from_secs(2);

/// Resolves a name and a port into addresses
pub type Lookup = Arc<dyn Fn(&str, u16) -> std::io::Result<Vec<SocketAddr>> + Send + Sync>;

/// The upstream a load balancer's backend came from (an index into the location's or `tcp`
/// block's upstreams)
#[derive(Clone, Copy)]
struct UpstreamIndex(usize);

/// The index of the upstream `backend` came from
pub fn upstream_index(backend: &Backend) -> Option<usize> {
    backend.ext.get::<UpstreamIndex>().map(|index| index.0)
}

/// Round-robin between the upstreams, in proportion to their weight (1 by default).
/// Names are resolved now, and again when the load balancer is updated.
pub fn load_balancer(upstreams: &[RouteUpstream]) -> LoadBalancer<RoundRobin> {
    load_balancer_with(
        upstreams,
        Arc::new(|name: &str, port: u16| Ok((name, port).to_socket_addrs()?.collect())),
    )
}

/// Like [`load_balancer`], resolving names with `lookup`
pub fn load_balancer_with(upstreams: &[RouteUpstream], lookup: Lookup) -> LoadBalancer<RoundRobin> {
    let resolver = Resolver::new(upstreams, lookup);
    let load_balancer = LoadBalancer::from_backends(Backends::new(Box::new(resolver)));
    // Loads the backends: the first discovery returns the addresses resolved above
    {
        let update = std::pin::pin!(load_balancer.update());
        let _ = update.poll(&mut Context::from_waker(Waker::noop()));
    }
    load_balancer
}

/// An upstream to resolve
struct Target {
    name: String,
    port: u16,
    weight: usize,
    /// Upstreams written as addresses are never resolved
    ip: Option<IpAddr>,
}

/// The discovery of a load balancer's backends: the upstreams' addresses, and the last
/// addresses of their names
struct Resolver {
    targets: Vec<Target>,
    addresses: Mutex<Vec<Vec<SocketAddr>>>,
    resolved_at: Mutex<Instant>,
    /// The first discovery returns the addresses resolved when built
    built: AtomicBool,
    lookup: Lookup,
}

impl Resolver {
    fn new(upstreams: &[RouteUpstream], lookup: Lookup) -> Self {
        let targets = upstreams
            .iter()
            .map(|upstream| Target {
                name: upstream.ip.to_string(),
                port: upstream.port,
                weight: upstream
                    .weight
                    .and_then(|weight| usize::try_from(weight).ok())
                    .unwrap_or(1),
                ip: upstream.ip.parse().ok(),
            })
            .collect::<Vec<_>>();
        let addresses = targets
            .iter()
            .map(|target| match target.ip {
                Some(ip) => vec![SocketAddr::new(ip, target.port)],
                None => lookup(&target.name, target.port).unwrap_or_else(|err| {
                    tracing::warn!(
                        "could not resolve upstream {}:{} (resolved again every few seconds): {err}",
                        target.name,
                        target.port
                    );
                    vec![]
                }),
            })
            .collect();
        Self {
            targets,
            addresses: Mutex::new(addresses),
            resolved_at: Mutex::new(Instant::now()),
            built: AtomicBool::new(true),
            lookup,
        }
    }

    /// Resolves the names again, unless they were just resolved
    async fn resolve(&self) {
        {
            let mut resolved_at = self.resolved_at.lock().unwrap_or_else(PoisonError::into_inner);
            if resolved_at.elapsed() < MIN_RESOLVE_INTERVAL {
                return;
            }
            *resolved_at = Instant::now();
        }

        for (index, target) in self.targets.iter().enumerate() {
            if target.ip.is_some() {
                continue;
            }
            let (lookup, name, port) = (self.lookup.clone(), target.name.clone(), target.port);
            let lookup = tokio::task::spawn_blocking(move || lookup(&name, port));
            let resolved = match tokio::time::timeout(LOOKUP_TIMEOUT, lookup).await {
                Ok(Ok(Ok(resolved))) if !resolved.is_empty() => resolved,
                Ok(Ok(Ok(_))) => continue,
                Ok(Ok(Err(err))) => {
                    tracing::debug!("could not resolve upstream {}: {err}", target.name);
                    continue;
                }
                _ => {
                    tracing::debug!("could not resolve upstream {} in time", target.name);
                    continue;
                }
            };
            let mut addresses = self.addresses.lock().unwrap_or_else(PoisonError::into_inner);
            if let Some(slot) = addresses.get_mut(index) {
                if *slot != resolved {
                    tracing::info!(
                        "upstream {}:{} now resolves to {resolved:?}",
                        target.name,
                        target.port
                    );
                    *slot = resolved;
                }
            }
        }
    }

    /// The backends of the addresses known, each with the upstream it came from
    fn backends(&self) -> BTreeSet<Backend> {
        let addresses = self.addresses.lock().unwrap_or_else(PoisonError::into_inner);
        let mut backends = BTreeSet::new();
        for (index, (target, addresses)) in self.targets.iter().zip(addresses.iter()).enumerate() {
            for addr in addresses {
                let mut ext = Extensions::new();
                ext.insert(UpstreamIndex(index));
                // The first upstream with an address keeps it
                backends.insert(Backend {
                    addr: BackendAddr::Inet(*addr),
                    weight: target.weight,
                    ext,
                });
            }
        }
        backends
    }
}

#[async_trait]
impl ServiceDiscovery for Resolver {
    async fn discover(
        &self,
    ) -> pingora::Result<(BTreeSet<Backend>, std::collections::HashMap<u64, bool>)> {
        if !self.built.swap(false, Ordering::Relaxed) {
            self.resolve().await;
        }
        Ok((self.backends(), std::collections::HashMap::new()))
    }
}

#[cfg(test)]
mod tests {
    use std::{
        borrow::Cow,
        collections::HashMap,
        net::SocketAddr,
        sync::{Arc, Mutex},
    };

    use pingora::lb::{selection::RoundRobin, LoadBalancer};

    use crate::config::RouteUpstream;

    use super::{load_balancer_with, upstream_index, Lookup};

    /// A DNS whose answers the test changes
    #[derive(Clone, Default)]
    struct Dns(Arc<Mutex<HashMap<String, Vec<&'static str>>>>);

    impl Dns {
        fn answer(&self, name: &str, addresses: &[&'static str]) {
            self.0
                .lock()
                .unwrap()
                .insert(name.to_string(), addresses.to_vec());
        }

        fn fail(&self, name: &str) {
            self.0.lock().unwrap().remove(name);
        }

        fn lookup(&self) -> Lookup {
            let answers = self.0.clone();
            Arc::new(move |name: &str, port: u16| {
                answers
                    .lock()
                    .unwrap()
                    .get(name)
                    .map(|addresses| {
                        addresses
                            .iter()
                            .map(|ip| SocketAddr::new(ip.parse().unwrap(), port))
                            .collect()
                    })
                    .ok_or_else(|| std::io::Error::other("no answer"))
            })
        }
    }

    fn upstream(ip: &'static str, port: u16) -> RouteUpstream {
        RouteUpstream {
            ip: Cow::Borrowed(ip),
            port,
            ..RouteUpstream::default()
        }
    }

    /// The addresses of the load balancer, with the upstream each one came from
    fn addresses(load_balancer: &LoadBalancer<RoundRobin>) -> Vec<(String, Option<usize>)> {
        load_balancer
            .backends()
            .get_backend()
            .iter()
            .map(|backend| (backend.addr.to_string(), upstream_index(backend)))
            .collect()
    }

    #[tokio::test]
    async fn test_addresses_and_names_are_resolved_when_built() {
        let dns = Dns::default();
        dns.answer("app", &["10.0.0.2", "10.0.0.3"]);

        let load_balancer =
            load_balancer_with(&[upstream("10.0.0.1", 80), upstream("app", 8080)], dns.lookup());

        assert_eq!(
            addresses(&load_balancer),
            vec![
                ("10.0.0.1:80".to_string(), Some(0)),
                ("10.0.0.2:8080".to_string(), Some(1)),
                ("10.0.0.3:8080".to_string(), Some(1)),
            ]
        );
    }

    #[tokio::test]
    async fn test_names_are_followed_when_their_addresses_change() {
        let dns = Dns::default();
        dns.answer("app", &["10.0.0.2"]);
        let load_balancer = load_balancer_with(&[upstream("app", 8080)], dns.lookup());

        dns.answer("app", &["10.0.0.9"]);
        tokio::time::sleep(super::MIN_RESOLVE_INTERVAL).await;
        load_balancer.update().await.unwrap();

        assert_eq!(addresses(&load_balancer), vec![("10.0.0.9:8080".to_string(), Some(0))]);
    }

    #[tokio::test]
    async fn test_failed_lookups_keep_the_last_addresses() {
        let dns = Dns::default();
        dns.answer("app", &["10.0.0.2"]);
        let load_balancer = load_balancer_with(&[upstream("app", 8080)], dns.lookup());

        dns.fail("app");
        tokio::time::sleep(super::MIN_RESOLVE_INTERVAL).await;
        load_balancer.update().await.unwrap();

        assert_eq!(addresses(&load_balancer), vec![("10.0.0.2:8080".to_string(), Some(0))]);
    }

    #[tokio::test]
    async fn test_names_that_dont_resolve_yet_are_resolved_later() {
        let dns = Dns::default();
        let load_balancer = load_balancer_with(&[upstream("later", 8080)], dns.lookup());
        assert!(addresses(&load_balancer).is_empty());

        dns.answer("later", &["10.0.0.4"]);
        tokio::time::sleep(super::MIN_RESOLVE_INTERVAL).await;
        load_balancer.update().await.unwrap();

        assert_eq!(addresses(&load_balancer), vec![("10.0.0.4:8080".to_string(), Some(0))]);
    }

    #[tokio::test]
    async fn test_names_are_resolved_at_most_once_a_second() {
        let dns = Dns::default();
        dns.answer("app", &["10.0.0.2"]);
        let load_balancer = load_balancer_with(&[upstream("app", 8080)], dns.lookup());

        // Many failed connections at once
        dns.answer("app", &["10.0.0.9"]);
        load_balancer.update().await.unwrap();

        assert_eq!(addresses(&load_balancer), vec![("10.0.0.2:8080".to_string(), Some(0))]);
    }
}
