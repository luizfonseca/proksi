//! From `Ingress`es (and the `Service`s and `EndpointSlice`s they use) to hosts (ADR 0005)

use std::{collections::BTreeMap, sync::Arc};

use k8s_openapi::api::{
    core::v1::Service,
    discovery::v1::EndpointSlice,
    networking::v1::{Ingress, IngressBackend, IngressClass, IngressServiceBackend},
};

use crate::config::{
    host::{Host, HostTls, Location},
    RouteUpstream,
};

/// Marks the default `IngressClass`, used by `Ingress`es without a class
const DEFAULT_CLASS: &str = "ingressclass.kubernetes.io/is-default-class";
/// `"false"`: the hosts of the `Ingress` are plain HTTP
const TLS: &str = "proksi.info/tls";
/// Links an `EndpointSlice` to its `Service`
const SERVICE_NAME: &str = "kubernetes.io/service-name";

/// What proksi watches in the cluster
#[derive(Debug, Default, Clone)]
pub struct Cluster {
    pub ingresses: Vec<Arc<Ingress>>,
    pub ingress_classes: Vec<Arc<IngressClass>>,
    pub services: Vec<Arc<Service>>,
    pub endpoint_slices: Vec<Arc<EndpointSlice>>,
}

/// An `Ingress` (or part of it) that couldn't be served
#[derive(Debug, Clone, PartialEq)]
pub struct Invalid {
    /// `namespace/name` of the `Ingress`
    pub ingress: String,
    pub reason: String,
}

/// A host's locations by path (and whether it's exact), with the `Ingress` defining each
type Locations = BTreeMap<(String, bool), (Location, String)>;

/// The hosts of the `Ingress`es of class `class`, and what couldn't be served
pub fn hosts(cluster: &Cluster, class: &str) -> (Vec<Host>, Vec<Invalid>) {
    let default_class = cluster.ingress_classes.iter().any(|ingress_class| {
        ingress_class.metadata.name.as_deref() == Some(class)
            && ingress_class
                .metadata
                .annotations
                .as_ref()
                .and_then(|annotations| annotations.get(DEFAULT_CLASS))
                .is_some_and(|value| value == "true")
    });
    let mut ingresses = cluster
        .ingresses
        .iter()
        .filter(|ingress| {
            match ingress.spec.as_ref().and_then(|spec| spec.ingress_class_name.as_deref()) {
                Some(name) => name == class,
                None => default_class,
            }
        })
        .collect::<Vec<_>>();
    // The oldest `Ingress` keeps a path defined twice
    ingresses.sort_by_key(|ingress| {
        (
            ingress.metadata.creation_timestamp.clone(),
            ingress.metadata.namespace.clone(),
            ingress.metadata.name.clone(),
        )
    });

    // Locations by host, with the `Ingress` defining each path, and whether the host has TLS
    // (as set by the oldest `Ingress` of the host)
    let mut hosts: BTreeMap<String, Locations> = BTreeMap::new();
    let mut tls: BTreeMap<String, bool> = BTreeMap::new();
    let mut invalid = vec![];

    for ingress in ingresses {
        let namespace = ingress.metadata.namespace.clone().unwrap_or_default();
        let id = format!(
            "{namespace}/{}",
            ingress.metadata.name.clone().unwrap_or_default()
        );
        let plain = ingress
            .metadata
            .annotations
            .as_ref()
            .and_then(|annotations| annotations.get(TLS))
            .is_some_and(|value| value == "false");
        for (host, path, exact, backend) in paths(ingress) {
            tls.entry(host.clone()).or_insert(!plain);
            let Some(service) = &backend.service else {
                invalid.push(Invalid {
                    ingress: id.clone(),
                    reason: format!(
                        "path {path} of host {host}: only service backends are supported"
                    ),
                });
                continue;
            };

            let locations = hosts.entry(host.clone()).or_default();
            if let Some((_, owner)) = locations.get(&(path.clone(), exact)) {
                invalid.push(Invalid {
                    ingress: id.clone(),
                    reason: format!("path {path} of host {host} is already defined by {owner}"),
                });
                continue;
            }

            let location = Location {
                path: path.clone(),
                exact,
                upstreams: upstreams(cluster, &namespace, service),
                headers: None,
                plugins: None,
                cache: None,
            };
            locations.insert((path, exact), (location, id.clone()));
        }
    }

    let hosts = hosts
        .into_iter()
        .map(|(name, locations)| Host {
            listen: vec![],
            tls: HostTls {
                enabled: tls.get(&name).copied().unwrap_or(true),
                ..HostTls::default()
            },
            locations: locations.into_values().map(|(location, _)| location).collect(),
            name,
        })
        .collect();
    (hosts, invalid)
}

/// The paths of an `Ingress`: host, path, whether it's exact, and backend
fn paths(ingress: &Ingress) -> Vec<(String, String, bool, &IngressBackend)> {
    let Some(spec) = &ingress.spec else {
        return vec![];
    };
    let mut paths = vec![];
    if let Some(backend) = &spec.default_backend {
        paths.push(("*".to_string(), "/".to_string(), false, backend));
    }
    for rule in spec.rules.iter().flatten() {
        let host = rule
            .host
            .clone()
            .filter(|host| !host.is_empty())
            .unwrap_or_else(|| "*".to_string());
        for path in rule.http.iter().flat_map(|http| &http.paths) {
            let exact = path.path_type == "Exact";
            let mut value = path.path.clone().unwrap_or_else(|| "/".to_string());
            // `Prefix` ignores a trailing slash (`/api/` is `/api`)
            if path.path_type == "Prefix" && value.len() > 1 {
                value = value.trim_end_matches('/').to_string();
            }
            paths.push((host.clone(), value, exact, &path.backend));
        }
    }
    paths
}

/// The ready endpoints of a service's port, in `namespace` (none if it doesn't exist)
fn upstreams(
    cluster: &Cluster,
    namespace: &str,
    backend: &IngressServiceBackend,
) -> Vec<RouteUpstream> {
    let Some(service) = cluster.services.iter().find(|service| {
        service.metadata.namespace.as_deref() == Some(namespace)
            && service.metadata.name.as_deref() == Some(backend.name.as_str())
    }) else {
        return vec![];
    };
    let ports = service
        .spec
        .as_ref()
        .and_then(|spec| spec.ports.as_ref())
        .map(Vec::as_slice)
        .unwrap_or_default();
    let wanted = backend.port.as_ref();
    let Some(port) = ports.iter().find(|port| match wanted {
        Some(wanted) if wanted.number.is_some() => wanted.number == Some(port.port),
        Some(wanted) => wanted.name.is_some() && wanted.name == port.name,
        None => ports.len() == 1,
    }) else {
        return vec![];
    };
    // Endpoint slices name their ports like the service's (unnamed for a single port)
    let port_name = port.name.clone().unwrap_or_default();

    let mut upstreams = vec![];
    let slices = cluster.endpoint_slices.iter().filter(|slice| {
        slice.metadata.namespace.as_deref() == Some(namespace)
            && slice
                .metadata
                .labels
                .as_ref()
                .and_then(|labels| labels.get(SERVICE_NAME))
                .is_some_and(|name| *name == backend.name)
    });
    for slice in slices {
        let Some(target) = slice
            .ports
            .iter()
            .flatten()
            .find(|port| port.name.clone().unwrap_or_default() == port_name)
            .and_then(|port| port.port)
            .and_then(|port| u16::try_from(port).ok())
        else {
            continue;
        };
        for endpoint in &slice.endpoints {
            // Ready unless said otherwise
            let ready = endpoint
                .conditions
                .as_ref()
                .and_then(|conditions| conditions.ready)
                .unwrap_or(true);
            if !ready {
                continue;
            }
            for address in &endpoint.addresses {
                let ip = if address.contains(':') {
                    format!("[{address}]")
                } else {
                    address.clone()
                };
                upstreams.push(RouteUpstream {
                    ip: ip.into(),
                    port: target,
                    ..RouteUpstream::default()
                });
            }
        }
    }
    upstreams
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use serde_json::{json, Value};

    use crate::config::host::Host;

    use super::{hosts, Cluster, Invalid};

    fn object<T: serde::de::DeserializeOwned>(value: Value) -> T {
        serde_json::from_value(value).unwrap()
    }

    fn ingress(name: &str, created: &str, spec: Value) -> k8s_openapi::api::networking::v1::Ingress {
        object(json!({
            "metadata": { "name": name, "namespace": "default", "creationTimestamp": created },
            "spec": spec,
        }))
    }

    fn rule(host: Option<&str>, paths: Value) -> Value {
        let mut rule = json!({ "http": { "paths": paths } });
        if let Some(host) = host {
            rule["host"] = json!(host);
        }
        rule
    }

    fn path(path: Option<&str>, path_type: &str, service: &str, port: Value) -> Value {
        let mut value = json!({
            "pathType": path_type,
            "backend": { "service": { "name": service, "port": port } },
        });
        if let Some(path) = path {
            value["path"] = json!(path);
        }
        value
    }

    /// Service `web` (port 80, named `http`, to the pods' 8080) with three pods, one not ready
    fn cluster(ingresses: Vec<k8s_openapi::api::networking::v1::Ingress>) -> Cluster {
        Cluster {
            ingresses: ingresses.into_iter().map(Arc::new).collect(),
            ingress_classes: vec![],
            services: vec![Arc::new(object(json!({
                "metadata": { "name": "web", "namespace": "default" },
                "spec": { "ports": [{ "name": "http", "port": 80, "targetPort": 8080 }] },
            })))],
            endpoint_slices: vec![Arc::new(object(json!({
                "metadata": {
                    "name": "web-abc",
                    "namespace": "default",
                    "labels": { "kubernetes.io/service-name": "web" },
                },
                "addressType": "IPv4",
                "ports": [{ "name": "http", "port": 8080 }],
                "endpoints": [
                    { "addresses": ["10.1.0.1"], "conditions": { "ready": true } },
                    { "addresses": ["10.1.0.2"], "conditions": { "ready": false } },
                    { "addresses": ["10.1.0.3"] },
                ],
            })))],
        }
    }

    fn upstreams(host: &Host, location: usize) -> Vec<String> {
        host.locations[location]
            .upstreams
            .iter()
            .map(|upstream| format!("{}:{}", upstream.ip, upstream.port))
            .collect()
    }

    fn by_name<'a>(hosts: &'a [Host], name: &str) -> &'a Host {
        hosts.iter().find(|host| host.name == name).unwrap()
    }

    #[test]
    fn test_rules_become_hosts_with_ready_pods() {
        let ingresses = vec![ingress(
            "web",
            "2026-01-01T00:00:00Z",
            json!({
                "ingressClassName": "proksi",
                "rules": [rule(Some("a.test"), json!([
                    path(Some("/api/"), "Prefix", "web", json!({ "number": 80 })),
                    path(Some("/health"), "Exact", "web", json!({ "name": "http" })),
                    path(Some("/users/:id"), "ImplementationSpecific", "web", json!({ "number": 80 })),
                ]))],
            }),
        )];

        let (hosts, invalid) = hosts(&cluster(ingresses), "proksi");

        assert_eq!(invalid, vec![]);
        assert_eq!(hosts.len(), 1);
        let host = &hosts[0];
        assert_eq!(host.name, "a.test");
        assert!(host.tls.enabled);
        let locations = host
            .locations
            .iter()
            .map(|location| (location.path.as_str(), location.exact))
            .collect::<Vec<_>>();
        assert_eq!(
            locations,
            vec![("/api", false), ("/health", true), ("/users/:id", false)]
        );
        // The pods' port, from the service port, and only ready pods
        for index in 0..3 {
            assert_eq!(upstreams(host, index), vec!["10.1.0.1:8080", "10.1.0.3:8080"]);
        }
    }

    #[test]
    fn test_only_ingresses_of_the_class() {
        let spec = |class: Option<&str>| {
            let mut spec = json!({
                "rules": [rule(Some("a.test"), json!([path(None, "Prefix", "web", json!({ "number": 80 }))]))],
            });
            if let Some(class) = class {
                spec["ingressClassName"] = json!(class);
            }
            spec
        };
        let mut cluster = cluster(vec![
            ingress("other", "2026-01-01T00:00:00Z", spec(Some("nginx"))),
            ingress("none", "2026-01-01T00:00:00Z", spec(None)),
        ]);
        assert_eq!(hosts(&cluster, "proksi").0, vec![]);

        // Without a class, when proksi's class is the default one
        cluster.ingress_classes = vec![Arc::new(object(json!({
            "metadata": {
                "name": "proksi",
                "annotations": { "ingressclass.kubernetes.io/is-default-class": "true" },
            },
            "spec": { "controller": "proksi.info/ingress-controller" },
        })))];
        let (hosts, _) = hosts(&cluster, "proksi");
        assert_eq!(hosts.len(), 1);
        assert_eq!(hosts[0].locations[0].path, "/");
    }

    #[test]
    fn test_ingresses_of_a_host_are_merged() {
        let ingresses = vec![
            ingress(
                "newer",
                "2026-02-01T00:00:00Z",
                json!({
                    "ingressClassName": "proksi",
                    "rules": [rule(Some("a.test"), json!([
                        path(Some("/api"), "Prefix", "web", json!({ "number": 80 })),
                        path(Some("/docs"), "Prefix", "web", json!({ "number": 80 })),
                    ]))],
                }),
            ),
            ingress(
                "older",
                "2026-01-01T00:00:00Z",
                json!({
                    "ingressClassName": "proksi",
                    "rules": [rule(Some("a.test"), json!([
                        path(Some("/api"), "Prefix", "missing", json!({ "number": 80 })),
                    ]))],
                }),
            ),
        ];

        let (hosts, invalid) = hosts(&cluster(ingresses), "proksi");

        let host = by_name(&hosts, "a.test");
        let paths = host
            .locations
            .iter()
            .map(|location| location.path.as_str())
            .collect::<Vec<_>>();
        assert_eq!(paths, vec!["/api", "/docs"]);
        // The older Ingress keeps the path (its service doesn't exist: no upstreams, a 503)
        assert_eq!(upstreams(host, 0), Vec::<String>::new());
        assert_eq!(
            invalid,
            vec![Invalid {
                ingress: "default/newer".to_string(),
                reason: "path /api of host a.test is already defined by default/older".to_string(),
            }]
        );
    }

    #[test]
    fn test_default_backend_and_rules_without_a_host() {
        let ingresses = vec![ingress(
            "catch-all",
            "2026-01-01T00:00:00Z",
            json!({
                "ingressClassName": "proksi",
                "defaultBackend": { "service": { "name": "web", "port": { "number": 80 } } },
                "rules": [rule(None, json!([path(Some("/api"), "Prefix", "web", json!({ "number": 80 }))]))],
            }),
        )];

        let (hosts, invalid) = hosts(&cluster(ingresses), "proksi");

        assert_eq!(invalid, vec![]);
        let host = by_name(&hosts, "*");
        let paths = host
            .locations
            .iter()
            .map(|location| location.path.as_str())
            .collect::<Vec<_>>();
        assert_eq!(paths, vec!["/", "/api"]);
    }

    #[test]
    fn test_unsupported_backends_and_ipv6() {
        let mut cluster = cluster(vec![ingress(
            "mixed",
            "2026-01-01T00:00:00Z",
            json!({
                "ingressClassName": "proksi",
                "rules": [rule(Some("a.test"), json!([
                    {
                        "path": "/bucket",
                        "pathType": "Prefix",
                        "backend": { "resource": { "kind": "StorageBucket", "name": "assets" } },
                    },
                    path(Some("/"), "Prefix", "web", json!({ "number": 80 })),
                ]))],
            }),
        )]);
        cluster.endpoint_slices.push(Arc::new(object(json!({
            "metadata": {
                "name": "web-v6",
                "namespace": "default",
                "labels": { "kubernetes.io/service-name": "web" },
            },
            "addressType": "IPv6",
            "ports": [{ "name": "http", "port": 8080 }],
            "endpoints": [{ "addresses": ["fd00::1"] }],
        }))));

        let (hosts, invalid) = hosts(&cluster, "proksi");

        assert_eq!(
            invalid,
            vec![Invalid {
                ingress: "default/mixed".to_string(),
                reason: "path /bucket of host a.test: only service backends are supported".to_string(),
            }]
        );
        assert_eq!(
            upstreams(&hosts[0], 0),
            vec!["10.1.0.1:8080", "10.1.0.3:8080", "[fd00::1]:8080"]
        );
    }

    #[test]
    fn test_services_of_other_namespaces_are_not_used() {
        let mut cluster = cluster(vec![ingress(
            "web",
            "2026-01-01T00:00:00Z",
            json!({
                "ingressClassName": "proksi",
                "rules": [rule(Some("a.test"), json!([path(Some("/"), "Prefix", "web", json!({ "number": 80 }))]))],
            }),
        )]);
        Arc::make_mut(&mut cluster.ingresses[0]).metadata.namespace = Some("other".to_string());

        let (hosts, _) = hosts(&cluster, "proksi");

        assert_eq!(upstreams(&hosts[0], 0), Vec::<String>::new());
    }

    #[test]
    fn test_plain_http_hosts() {
        let mut cluster = cluster(vec![ingress(
            "plain",
            "2026-01-01T00:00:00Z",
            json!({
                "ingressClassName": "proksi",
                "rules": [rule(Some("a.test"), json!([path(Some("/"), "Prefix", "web", json!({ "number": 80 }))]))],
            }),
        )]);
        Arc::make_mut(&mut cluster.ingresses[0]).metadata.annotations = Some(
            [("proksi.info/tls".to_string(), "false".to_string())]
                .into_iter()
                .collect(),
        );

        let (hosts, _) = hosts(&cluster, "proksi");

        assert!(!hosts[0].tls.enabled);
    }
}
