//! What the proxy needs to serve a host

use crate::{config::host::HostTls, routing::paths::LocationMatcher};

use super::routes::RouteStoreContainer;

/// A host being served: its locations, chosen by path
pub struct HostContainer {
    pub locations: Vec<RouteStoreContainer>,
    matcher: LocationMatcher,
    /// Served over HTTPS (otherwise over plain HTTP, without certificates)
    pub tls_enabled: bool,
    pub self_signed_certificate: bool,
}

impl HostContainer {
    /// `locations`: the path patterns of each location, with its container
    pub fn new(locations: Vec<(Vec<String>, RouteStoreContainer)>, tls: &HostTls) -> Self {
        let (patterns, locations): (Vec<_>, Vec<_>) = locations.into_iter().unzip();
        Self {
            locations,
            matcher: LocationMatcher::new(patterns),
            tls_enabled: tls.enabled,
            self_signed_certificate: tls.self_signed_on_failure,
        }
    }

    /// The location serving `path`: its most specific match
    pub fn location_for(&self, path: &str) -> Option<&RouteStoreContainer> {
        self.locations.get(self.matcher.find(path)?)
    }
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
}
