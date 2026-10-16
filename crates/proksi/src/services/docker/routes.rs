//! Routes defined by docker labels

use std::{borrow::Cow, collections::HashMap, net::ToSocketAddrs};

use crate::config::{
    host::Host, Route, RouteHeader, RouteMatcher, RoutePathMatcher, RouteSslCertificate,
    RouteUpstream,
};

use super::ProksiDockerRoute;

/// Converts the labels of a docker container/service into a route
pub fn docker_route(host: &str, docker_route: ProksiDockerRoute) -> Route {
    let upstreams = docker_route
        .upstreams
        .iter()
        .filter_map(|upstream| upstream.to_socket_addrs().ok())
        .flatten()
        .map(|addr| RouteUpstream {
            ip: Cow::Owned(addr.ip().to_string()),
            port: addr.port(),
            weight: Some(1),
            ..RouteUpstream::default()
        })
        .collect();

    let match_with = (!docker_route.path_matchers.is_empty()).then(|| RouteMatcher {
        path: Some(RoutePathMatcher {
            patterns: docker_route
                .path_matchers
                .into_iter()
                .map(Cow::Owned)
                .collect(),
        }),
    });

    Route {
        host: Cow::Owned(host.to_string()),
        upstreams,
        match_with,
        headers: Some(RouteHeader {
            add: Some(docker_route.host_header_add.unwrap_or_default()),
            remove: Some(docker_route.host_header_remove.unwrap_or_default()),
        }),
        plugins: Some(docker_route.plugins.unwrap_or_default()),
        ssl_certificate: Some(RouteSslCertificate {
            self_signed_on_failure: Some(docker_route.ssl_certificate_self_signed_on_failure),
        }),
        cache: None,
        ssl: None,
    }
}

/// Routes found in docker. Routes of containers that disappeared are kept: the labels
/// are the route's definition, so the route stays (and answers with a 503).
#[derive(Default)]
pub struct DockerRoutes {
    routes: HashMap<String, Route>,
}

impl DockerRoutes {
    /// Updates the routes with what was found in docker and returns all known hosts
    pub fn update(&mut self, found: HashMap<String, ProksiDockerRoute>) -> Vec<Host> {
        for (host, found_route) in found {
            // Without upstreams (e.g. no reachable network), keep the previous route
            if found_route.upstreams.is_empty() {
                continue;
            }
            self.routes
                .insert(host.clone(), docker_route(&host, found_route));
        }

        self.routes.values().map(Host::from_route).collect()
    }
}

#[cfg(test)]
mod tests {
    use std::{borrow::Cow, collections::HashMap};

    use crate::config::{host::Host, RouteHeaderAdd, RouteHeaderRemove, RoutePlugin};

    use super::{docker_route, DockerRoutes, ProksiDockerRoute};

    fn found(upstreams: &[&str]) -> ProksiDockerRoute {
        ProksiDockerRoute::new(upstreams.iter().map(ToString::to_string).collect(), vec![])
    }

    fn hosts_and_upstreams(hosts: &[crate::config::host::Host]) -> Vec<(String, Vec<String>)> {
        let mut result = hosts
            .iter()
            .map(|h| {
                (
                    h.name.clone(),
                    h.locations[0]
                        .upstreams
                        .iter()
                        .map(|u| format!("{}:{}", u.ip, u.port))
                        .collect(),
                )
            })
            .collect::<Vec<_>>();
        result.sort();
        result
    }

    #[test]
    fn test_docker_route() {
        let mut found = found(&["10.0.0.1:3000", "10.0.0.2:3000"]);
        found.path_matchers = vec!["/api/*".to_string()];
        found.host_header_add = Some(vec![RouteHeaderAdd {
            name: Cow::Borrowed("x-added"),
            value: Cow::Borrowed("yes"),
        }]);
        found.host_header_remove = Some(vec![RouteHeaderRemove {
            name: Cow::Borrowed("server"),
        }]);
        found.plugins = Some(vec![RoutePlugin {
            name: Cow::Borrowed("request_id"),
            config: None,
        }]);
        found.ssl_certificate_self_signed_on_failure = true;

        let route = docker_route("app.test", found);

        assert_eq!(route.host, "app.test");
        assert_eq!(
            hosts_and_upstreams(&[Host::from_route(&route)]),
            vec![(
                "app.test".to_string(),
                vec!["10.0.0.1:3000".to_string(), "10.0.0.2:3000".to_string()]
            )]
        );
        assert_eq!(route.upstreams[0].weight, Some(1));
        assert_eq!(
            route.match_with.unwrap().path.unwrap().patterns,
            vec![Cow::Borrowed("/api/*")]
        );
        let headers = route.headers.unwrap();
        assert_eq!(headers.add.unwrap()[0].name, "x-added");
        assert_eq!(headers.remove.unwrap()[0].name, "server");
        assert_eq!(route.plugins.unwrap()[0].name, "request_id");
        assert_eq!(
            route.ssl_certificate.unwrap().self_signed_on_failure,
            Some(true)
        );
    }

    #[test]
    fn test_docker_route_without_path_matchers() {
        let route = docker_route("app.test", found(&["10.0.0.1:3000"]));

        assert!(route.match_with.is_none());
    }

    #[test]
    fn test_docker_routes_are_kept_when_containers_disappear() {
        let mut routes = DockerRoutes::default();
        routes.update(HashMap::from([
            ("a.test".to_string(), found(&["10.0.0.1:3000"])),
            ("b.test".to_string(), found(&["10.0.0.2:3000"])),
        ]));

        let current = routes.update(HashMap::from([(
            "b.test".to_string(),
            found(&["10.0.0.3:3000"]),
        )]));

        assert_eq!(
            hosts_and_upstreams(&current),
            vec![
                ("a.test".to_string(), vec!["10.0.0.1:3000".to_string()]),
                ("b.test".to_string(), vec!["10.0.0.3:3000".to_string()]),
            ]
        );
    }

    #[test]
    fn test_docker_routes_without_upstreams_keep_the_previous_route() {
        let mut routes = DockerRoutes::default();
        routes.update(HashMap::from([(
            "a.test".to_string(),
            found(&["10.0.0.1:3000"]),
        )]));

        let current = routes.update(HashMap::from([("a.test".to_string(), found(&[]))]));

        assert_eq!(
            hosts_and_upstreams(&current),
            vec![("a.test".to_string(), vec!["10.0.0.1:3000".to_string()])]
        );
    }

    #[test]
    fn test_docker_routes_without_upstreams_are_not_added() {
        let mut routes = DockerRoutes::default();

        let current = routes.update(HashMap::from([("a.test".to_string(), found(&[]))]));

        assert!(current.is_empty());
    }
}
