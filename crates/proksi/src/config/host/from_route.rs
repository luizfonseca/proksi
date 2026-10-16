//! Routes (`routes = [...]`, docker labels, stored routes with schema version 1) as hosts

use std::collections::HashSet;

use crate::config::Route;

use super::{Host, HostTls, Location};

impl Host {
    /// Converts a route. Each path pattern becomes an exact location with the route's
    /// settings, or `/*` without patterns (routes matched their patterns exactly).
    pub fn from_route(route: &Route) -> Self {
        let mut seen = HashSet::new();
        let mut paths = route
            .match_with
            .as_ref()
            .and_then(|matcher| matcher.path.as_ref())
            .map(|path| {
                path.patterns
                    .iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        paths.retain(|path| seen.insert(path.clone()));
        if paths.is_empty() {
            paths.push("/*".to_string());
        }

        Self {
            name: route.host.to_string(),
            tls: HostTls {
                enabled: true,
                certificate: route.ssl.as_ref().and_then(|ssl| ssl.path.clone()),
                self_signed_on_failure: route
                    .ssl_certificate
                    .as_ref()
                    .and_then(|ssl| ssl.self_signed_on_failure)
                    .unwrap_or(false),
            },
            locations: paths
                .into_iter()
                .map(|path| Location {
                    path,
                    exact: true,
                    upstreams: route.upstreams.clone(),
                    headers: route.headers.clone(),
                    plugins: route.plugins.clone(),
                    cache: route.cache.clone(),
                })
                .collect(),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use crate::{
        config::{
            host::{Host, HostTls},
            RouteSslPath,
        },
        routing::test_utils::route,
    };

    #[test]
    fn test_host_from_route_without_patterns() {
        let mut route = route("A.test", "10.0.0.1");
        route.ssl = Some(
            serde_json::from_value(serde_json::json!({
                "path": { "key": "/certs/a.key", "pem": "/certs/a.pem" }
            }))
            .unwrap(),
        );

        let host = Host::from_route(&route);

        assert_eq!(host.name, "A.test");
        assert_eq!(
            host.tls,
            HostTls {
                enabled: true,
                certificate: Some(RouteSslPath {
                    key: PathBuf::from("/certs/a.key"),
                    pem: PathBuf::from("/certs/a.pem"),
                }),
                self_signed_on_failure: false,
            }
        );
        assert_eq!(host.locations.len(), 1);
        assert_eq!(host.locations[0].path, "/*");
        assert_eq!(host.locations[0].upstreams, route.upstreams);
    }

    #[test]
    fn test_host_from_route_with_patterns_keeps_exact_matching() {
        let mut route = route("a.test", "10.0.0.1");
        route.match_with = Some(
            serde_json::from_value(serde_json::json!({
                "path": { "patterns": ["/api/*", "/auth", "/auth"] }
            }))
            .unwrap(),
        );
        route.ssl_certificate = Some(
            serde_json::from_value(serde_json::json!({ "self_signed_on_failure": true })).unwrap(),
        );

        let host = Host::from_route(&route);

        let paths = host
            .locations
            .iter()
            .map(|l| (l.path.as_str(), l.exact))
            .collect::<Vec<_>>();
        assert_eq!(paths, vec![("/api/*", true), ("/auth", true)]);
        assert!(host
            .locations
            .iter()
            .all(|l| l.upstreams == route.upstreams));
        assert!(host.tls.self_signed_on_failure);
    }
}
