//! What the proxy needs to serve a host

use std::{net::SocketAddr, sync::Arc};

use crate::{config::host::HostTls, routing::paths::LocationMatcher};

use super::routes::RouteStoreContainer;

/// A host being served: its locations, chosen by path
pub struct HostContainer {
    /// Shared with the requests they serve
    pub locations: Vec<Arc<RouteStoreContainer>>,
    matcher: LocationMatcher,
    /// Served over HTTPS (otherwise over plain HTTP, without certificates)
    pub tls_enabled: bool,
    /// Addresses the host is served on (empty: the default address of its protocol)
    pub listen: Vec<SocketAddr>,
    pub self_signed_certificate: bool,
}

impl HostContainer {
    /// `locations`: the path patterns of each location, with its container
    pub fn new(locations: Vec<(Vec<String>, RouteStoreContainer)>, tls: &HostTls) -> Self {
        let (patterns, locations): (Vec<_>, Vec<_>) = locations.into_iter().unzip();
        Self {
            locations: locations.into_iter().map(Arc::new).collect(),
            matcher: LocationMatcher::new(patterns),
            tls_enabled: tls.enabled,
            listen: vec![],
            self_signed_certificate: tls.self_signed_on_failure,
        }
    }

    /// Port to redirect HTTP requests to, when the host is served over HTTPS on another
    /// port than 443 (its first `listen` address)
    pub fn https_redirect_port(&self) -> Option<u16> {
        self.listen
            .first()
            .map(SocketAddr::port)
            .filter(|port| *port != 443)
    }

    /// Whether the host is served on `local`, the address a request came in on
    /// (`default`: the default address of the listener's protocol)
    pub fn listens_on(&self, local: Option<&SocketAddr>, default: &SocketAddr) -> bool {
        let Some(local) = local else {
            // Unknown (e.g. unix sockets): can't tell, serve it
            return true;
        };
        if self.listen.is_empty() {
            return binds(default, local);
        }
        self.listen.iter().any(|address| binds(address, local))
    }

    /// The location serving `path`: its most specific match
    pub fn location_for(&self, path: &str) -> Option<&Arc<RouteStoreContainer>> {
        self.locations.get(self.matcher.find(path)?)
    }
}

/// Whether a listener bound to `bound` receives connections on `local`
/// (`0.0.0.0`/`::` receive on every local address of the port)
pub(crate) fn binds(bound: &SocketAddr, local: &SocketAddr) -> bool {
    bound.port() == local.port() && (bound.ip().is_unspecified() || bound.ip() == local.ip())
}

#[cfg(test)]
mod tests {
    use crate::{
        config::{host::HostTls, RouteUpstream},
        stores::routes::RouteStoreContainer,
    };

    use super::HostContainer;

    fn location(ip: &str) -> RouteStoreContainer {
        let mut container = RouteStoreContainer::default();
        container.set_upstreams(vec![RouteUpstream {
            ip: ip.to_string().into(),
            port: 3000,
            ..Default::default()
        }]);
        container
    }

    #[test]
    fn test_tls_settings() {
        let host = HostContainer::new(
            vec![],
            &HostTls {
                enabled: false,
                certificate: None,
                self_signed_on_failure: true,
            },
        );
        assert!(!host.tls_enabled);
        assert!(host.self_signed_certificate);
    }

    #[test]
    fn test_location_for_path() {
        let host = HostContainer::new(
            vec![
                (vec!["/*".to_string()], location("10.0.0.1")),
                (
                    vec!["/api".to_string(), "/api/*".to_string()],
                    location("10.0.0.2"),
                ),
            ],
            &HostTls {
                self_signed_on_failure: true,
                ..HostTls::default()
            },
        );
        let served_by = |path: &str| {
            host.location_for(path)
                .map(|location| location.upstreams[0].ip.to_string())
        };

        assert_eq!(served_by("/"), Some("10.0.0.1".to_string()));
        assert_eq!(served_by("/api/users"), Some("10.0.0.2".to_string()));
        assert_eq!(host.locations.len(), 2);
        assert!(host.self_signed_certificate);
    }

    #[test]
    fn test_no_location_for_path() {
        let host = HostContainer::new(
            vec![(vec!["/api".to_string()], location("10.0.0.1"))],
            &HostTls::default(),
        );
        assert!(host.location_for("/other").is_none());
    }

    #[test]
    fn test_location_is_shared_between_requests() {
        let host = HostContainer::new(
            vec![(vec!["/*".to_string()], location("10.0.0.1"))],
            &HostTls::default(),
        );

        assert!(std::sync::Arc::ptr_eq(
            host.location_for("/a").unwrap(),
            host.location_for("/b").unwrap()
        ));
    }

    fn listening_on(listen: &[&str]) -> HostContainer {
        let mut host = HostContainer::new(vec![], &HostTls::default());
        host.listen = listen.iter().map(|a| a.parse().unwrap()).collect();
        host
    }

    #[test]
    fn test_host_without_listen_uses_the_default_address() {
        let host = listening_on(&[]);
        let default = "0.0.0.0:443".parse().unwrap();

        assert!(host.listens_on(Some(&"10.0.0.1:443".parse().unwrap()), &default));
        assert!(!host.listens_on(Some(&"10.0.0.1:8443".parse().unwrap()), &default));
        // Unknown local address (e.g. unix sockets): served
        assert!(host.listens_on(None, &default));
    }

    #[test]
    fn test_host_listening_on_its_addresses() {
        let host = listening_on(&["0.0.0.0:8080", "127.0.0.1:9090"]);
        let default = "0.0.0.0:80".parse().unwrap();

        assert!(host.listens_on(Some(&"192.168.1.10:8080".parse().unwrap()), &default));
        assert!(host.listens_on(Some(&"127.0.0.1:9090".parse().unwrap()), &default));
        assert!(!host.listens_on(Some(&"192.168.1.10:9090".parse().unwrap()), &default));
        assert!(!host.listens_on(Some(&"127.0.0.1:80".parse().unwrap()), &default));
    }

    #[test]
    fn test_https_redirect_port() {
        assert_eq!(listening_on(&[]).https_redirect_port(), None);
        assert_eq!(listening_on(&["0.0.0.0:443"]).https_redirect_port(), None);
        assert_eq!(
            listening_on(&["127.0.0.1:8443", "0.0.0.0:9443"]).https_redirect_port(),
            Some(8443)
        );
    }
}
