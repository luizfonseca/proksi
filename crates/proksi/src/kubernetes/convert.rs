//! From `Ingress`es (and the `Service`s and `EndpointSlice`s they use) to hosts (ADR 0005)

use std::{collections::BTreeMap, sync::Arc};

use k8s_openapi::api::{
    core::v1::{Secret, Service},
    discovery::v1::EndpointSlice,
    networking::v1::{Ingress, IngressBackend, IngressClass, IngressServiceBackend},
};

use crate::config::{
    blocks::LocationBlock,
    hcl::HclFormat,
    host::{Host, HostTls, Location},
    RouteUpstream,
};

/// Marks the default `IngressClass`, used by `Ingress`es without a class
const DEFAULT_CLASS: &str = "ingressclass.kubernetes.io/is-default-class";
/// The class of an `Ingress` before `spec.ingressClassName`
const CLASS_ANNOTATION: &str = "kubernetes.io/ingress.class";
/// `"false"`: the hosts of the `Ingress` are plain HTTP
const TLS: &str = "proksi.ingress/tls";
/// Plugins, headers and cache, written like the inside of a `location` block
const BLOCKS: &str = "proksi.ingress/blocks";
/// A `Secret` (labelled `proksi.ingress/auth`) with the settings of `basic_auth` and `oauth2`
const AUTH_SECRET: &str = "proksi.ingress/auth-secret";
/// The label auth secrets need, so only those are watched
pub const AUTH_LABEL: &str = "proksi.ingress/auth";
/// `"service"`: upstreams are the services' cluster IPs, balanced by Kubernetes
const UPSTREAM: &str = "proksi.ingress/upstream";
/// Links an `EndpointSlice` to its `Service`
const SERVICE_NAME: &str = "kubernetes.io/service-name";

/// What proksi watches in the cluster
#[derive(Debug, Default, Clone)]
pub struct Cluster {
    pub ingresses: Vec<Arc<Ingress>>,
    pub ingress_classes: Vec<Arc<IngressClass>>,
    pub services: Vec<Arc<Service>>,
    pub endpoint_slices: Vec<Arc<EndpointSlice>>,
    pub secrets: Vec<Arc<Secret>>,
}

/// What the `Ingress`es of proksi's class turn into
#[derive(Debug, Default)]
pub struct Converted {
    pub hosts: Vec<Host>,
    /// The certificate of each host listed in an `Ingress`' `tls`
    pub certificates: Vec<HostCertificate>,
    pub invalid: Vec<Invalid>,
}

/// A certificate from a `Secret` (`tls.crt`, `tls.key`) for a host
#[derive(Debug, Clone, PartialEq)]
pub struct HostCertificate {
    pub host: String,
    /// `namespace/name` of the `Secret`, and its version (to tell when it changed)
    pub secret: String,
    pub version: Option<String>,
    pub certificate: Vec<u8>,
    pub key: Vec<u8>,
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
pub fn hosts(cluster: &Cluster, class: &str) -> Converted {
    let ingresses = of_class(cluster, class);

    // Locations by host, with the `Ingress` defining each path, and whether the host has TLS
    // (as set by the oldest `Ingress` of the host)
    let mut hosts: BTreeMap<String, Locations> = BTreeMap::new();
    let mut tls: BTreeMap<String, bool> = BTreeMap::new();
    // The certificate of each host, from the oldest `Ingress` listing it
    let mut certificates: BTreeMap<String, HostCertificate> = BTreeMap::new();
    let mut invalid = vec![];

    for ingress in ingresses {
        let namespace = ingress.metadata.namespace.clone().unwrap_or_default();
        let id = format!(
            "{namespace}/{}",
            ingress.metadata.name.clone().unwrap_or_default()
        );
        let plain = annotation(ingress, TLS) == Some("false");
        let through_service = annotation(ingress, UPSTREAM) == Some("service");
        // Plugins, headers and cache for every path of the `Ingress`
        let blocks = match blocks(cluster, ingress, &namespace) {
            Ok(blocks) => blocks,
            Err(reason) => {
                invalid.push(Invalid { ingress: id, reason });
                continue;
            }
        };

        tls_certificates(cluster, ingress, &id, &mut certificates, &mut invalid);

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
                upstreams: upstreams(cluster, &namespace, service, through_service),
                headers: blocks.as_ref().and_then(|blocks| blocks.headers.clone()),
                plugins: blocks.as_ref().and_then(|blocks| blocks.plugins.clone()),
                cache: blocks.as_ref().and_then(|blocks| blocks.cache.clone()),
                timeouts: blocks.as_ref().and_then(|blocks| blocks.timeouts),
                rewrite: blocks.as_ref().and_then(|blocks| blocks.rewrite.clone()),
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
            locations: locations
                .into_values()
                .map(|(location, _)| location)
                .collect(),
            name,
        })
        .collect();
    Converted {
        hosts,
        certificates: certificates.into_values().collect(),
        invalid,
    }
}

/// The `Ingress`es of class `class` (or without a class, when `class` is the default one),
/// oldest first: the oldest keeps a path (or a certificate) defined twice
pub fn of_class<'a>(cluster: &'a Cluster, class: &str) -> Vec<&'a Arc<Ingress>> {
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
            // `spec.ingressClassName`, or the annotation it replaced (still set by some tools,
            // like cert-manager's HTTP-01 solver with `class`)
            let name = ingress
                .spec
                .as_ref()
                .and_then(|spec| spec.ingress_class_name.as_deref())
                .or_else(|| annotation(ingress, CLASS_ANNOTATION));
            match name {
                Some(name) => name == class,
                None => default_class,
            }
        })
        .collect::<Vec<_>>();
    ingresses.sort_by_key(|ingress| {
        (
            ingress.metadata.creation_timestamp.clone(),
            ingress.metadata.namespace.clone(),
            ingress.metadata.name.clone(),
        )
    });

    ingresses
}

/// The certificates of the hosts listed in an `Ingress`' `tls`, unless an older one listed them
fn tls_certificates(
    cluster: &Cluster,
    ingress: &Ingress,
    id: &str,
    certificates: &mut BTreeMap<String, HostCertificate>,
    invalid: &mut Vec<Invalid>,
) {
    let namespace = ingress.metadata.namespace.clone().unwrap_or_default();
    for entry in ingress
        .spec
        .iter()
        .flat_map(|spec| spec.tls.iter().flatten())
    {
        let Some(name) = &entry.secret_name else {
            continue;
        };
        let secret = format!("{namespace}/{name}");
        for host in entry.hosts.iter().flatten() {
            if certificates.contains_key(host) {
                continue;
            }
            match tls_secret(cluster, &namespace, name) {
                Some((version, certificate, key)) => {
                    certificates.insert(
                        host.clone(),
                        HostCertificate {
                            host: host.clone(),
                            secret: secret.clone(),
                            version,
                            certificate,
                            key,
                        },
                    );
                }
                None => invalid.push(Invalid {
                    ingress: id.to_string(),
                    reason: format!("TLS secret {secret} (for {host}) not found"),
                }),
            }
        }
    }
}

/// The blocks of an `Ingress` (`proksi.ingress/blocks`), as a location without upstreams, with the
/// settings of its auth secret
fn blocks(cluster: &Cluster, ingress: &Ingress, namespace: &str) -> Result<Option<Location>, String> {
    let Some(text) = annotation(ingress, BLOCKS) else {
        return Ok(None);
    };
    let block = <HclFormat as figment::providers::Format>::from_str::<LocationBlock>(text)
        .map_err(|err| format!("invalid {BLOCKS}: {err}"))?;
    if !block.upstream.is_empty() {
        return Err(format!(
            "{BLOCKS} can't have upstreams: they come from the Ingress' backends"
        ));
    }
    let mut location = block
        .to_location("/", None, None)
        .map_err(|err| format!("invalid {BLOCKS}: {err}"))?;

    if let Some(name) = annotation(ingress, AUTH_SECRET) {
        let data = cluster
            .secrets
            .iter()
            .find(|secret| {
                secret.metadata.namespace.as_deref() == Some(namespace)
                    && secret.metadata.name.as_deref() == Some(name)
            })
            .and_then(|secret| secret.data.as_ref())
            .ok_or_else(|| {
                format!("auth secret {namespace}/{name} not found (it needs the label {AUTH_LABEL})")
            })?;
        let auth = location
            .plugins
            .iter_mut()
            .flatten()
            .filter(|plugin| plugin.name == "basic_auth" || plugin.name == "oauth2");
        for plugin in auth {
            let config = plugin.config.get_or_insert_with(Default::default);
            for (key, value) in data {
                let value = String::from_utf8_lossy(&value.0).into_owned();
                config
                    .entry(key.clone().into())
                    .or_insert(serde_json::Value::String(value));
            }
        }
    }
    Ok(Some(location))
}

fn annotation<'a>(ingress: &'a Ingress, name: &str) -> Option<&'a str> {
    ingress
        .metadata
        .annotations
        .as_ref()
        .and_then(|annotations| annotations.get(name))
        .map(String::as_str)
}

/// The version, certificate and key of a TLS `Secret`
fn tls_secret(
    cluster: &Cluster,
    namespace: &str,
    name: &str,
) -> Option<(Option<String>, Vec<u8>, Vec<u8>)> {
    let secret = cluster.secrets.iter().find(|secret| {
        secret.metadata.namespace.as_deref() == Some(namespace)
            && secret.metadata.name.as_deref() == Some(name)
    })?;
    let data = secret.data.as_ref()?;
    Some((
        secret.metadata.resource_version.clone(),
        data.get("tls.crt")?.0.clone(),
        data.get("tls.key")?.0.clone(),
    ))
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
    through_service: bool,
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
    // The service's address, unless it has none (headless services)
    let cluster_ip = service
        .spec
        .as_ref()
        .and_then(|spec| spec.cluster_ip.as_deref())
        .filter(|ip| !ip.is_empty() && *ip != "None");
    if let (true, Some(ip), Ok(port)) = (through_service, cluster_ip, u16::try_from(port.port)) {
        return vec![RouteUpstream {
            ip: if ip.contains(':') {
                format!("[{ip}]").into()
            } else {
                ip.to_string().into()
            },
            port,
            ..RouteUpstream::default()
        }];
    }

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

    use super::{hosts, Cluster, Converted, HostCertificate, Invalid};

    fn object<T: serde::de::DeserializeOwned>(value: Value) -> T {
        serde_json::from_value(value).unwrap()
    }

    fn ingress(
        name: &str,
        created: &str,
        spec: Value,
    ) -> k8s_openapi::api::networking::v1::Ingress {
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
            secrets: vec![Arc::new(object(json!({
                "metadata": { "name": "web-tls", "namespace": "default", "resourceVersion": "7" },
                "type": "kubernetes.io/tls",
                // "certificate" and "key", base64
                "data": { "tls.crt": "Y2VydGlmaWNhdGU=", "tls.key": "a2V5" },
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

        let Converted { hosts, invalid, .. } = hosts(&cluster(ingresses), "proksi");

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
            assert_eq!(
                upstreams(host, index),
                vec!["10.1.0.1:8080", "10.1.0.3:8080"]
            );
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
        assert_eq!(hosts(&cluster, "proksi").hosts, vec![]);

        // Without a class, when proksi's class is the default one
        cluster.ingress_classes = vec![Arc::new(object(json!({
            "metadata": {
                "name": "proksi",
                "annotations": { "ingressclass.kubernetes.io/is-default-class": "true" },
            },
            "spec": { "controller": "proksi.ingress/controller" },
        })))];
        let Converted { hosts, .. } = hosts(&cluster, "proksi");
        assert_eq!(hosts.len(), 1);
        assert_eq!(hosts[0].locations[0].path, "/");
    }

    #[test]
    fn test_the_class_annotation_is_respected() {
        let annotated = |name: &str, class: &str| {
            let mut ingress = ingress(
                name,
                "2026-01-01T00:00:00Z",
                json!({
                    "rules": [rule(Some(&format!("{name}.test")), json!([path(None, "Prefix", "web", json!({ "number": 80 }))]))],
                }),
            );
            ingress.metadata.annotations = Some(
                [("kubernetes.io/ingress.class".to_string(), class.to_string())].into(),
            );
            ingress
        };
        let mut cluster = cluster(vec![annotated("mine", "proksi"), annotated("theirs", "traefik")]);
        // Even when proksi's class is the default one, Ingresses annotated for another aren't served
        cluster.ingress_classes = vec![Arc::new(object(json!({
            "metadata": {
                "name": "proksi",
                "annotations": { "ingressclass.kubernetes.io/is-default-class": "true" },
            },
            "spec": { "controller": "proksi.ingress/controller" },
        })))];

        let names = hosts(&cluster, "proksi")
            .hosts
            .into_iter()
            .map(|host| host.name)
            .collect::<Vec<_>>();
        assert_eq!(names, vec!["mine.test"]);
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

        let Converted { hosts, invalid, .. } = hosts(&cluster(ingresses), "proksi");

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

        let Converted { hosts, invalid, .. } = hosts(&cluster(ingresses), "proksi");

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

        let Converted { hosts, invalid, .. } = hosts(&cluster, "proksi");

        assert_eq!(
            invalid,
            vec![Invalid {
                ingress: "default/mixed".to_string(),
                reason: "path /bucket of host a.test: only service backends are supported"
                    .to_string(),
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

        let Converted { hosts, .. } = hosts(&cluster, "proksi");

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
        Arc::make_mut(&mut cluster.ingresses[0])
            .metadata
            .annotations = Some(
            [("proksi.ingress/tls".to_string(), "false".to_string())]
                .into_iter()
                .collect(),
        );

        let Converted { hosts, .. } = hosts(&cluster, "proksi");

        assert!(!hosts[0].tls.enabled);
    }

    #[test]
    fn test_certificates_from_secrets() {
        let cluster = cluster(vec![ingress(
            "web",
            "2026-01-01T00:00:00Z",
            json!({
                "ingressClassName": "proksi",
                "tls": [
                    { "hosts": ["a.test", "b.test"], "secretName": "web-tls" },
                    { "hosts": ["c.test"], "secretName": "missing" },
                ],
                "rules": [rule(Some("a.test"), json!([path(Some("/"), "Prefix", "web", json!({ "number": 80 }))]))],
            }),
        )]);

        let Converted {
            certificates,
            invalid,
            ..
        } = hosts(&cluster, "proksi");

        let certificate = |host: &str| HostCertificate {
            host: host.to_string(),
            secret: "default/web-tls".to_string(),
            version: Some("7".to_string()),
            certificate: b"certificate".to_vec(),
            key: b"key".to_vec(),
        };
        assert_eq!(
            certificates,
            vec![certificate("a.test"), certificate("b.test")]
        );
        assert_eq!(
            invalid,
            vec![Invalid {
                ingress: "default/web".to_string(),
                reason: "TLS secret default/missing (for c.test) not found".to_string(),
            }]
        );
    }

    #[test]
    fn test_upstream_through_the_service() {
        let mut cluster = cluster(vec![ingress(
            "web",
            "2026-01-01T00:00:00Z",
            json!({
                "ingressClassName": "proksi",
                "rules": [rule(Some("a.test"), json!([path(Some("/"), "Prefix", "web", json!({ "number": 80 }))]))],
            }),
        )]);
        Arc::make_mut(&mut cluster.ingresses[0])
            .metadata
            .annotations = Some(
            [("proksi.ingress/upstream".to_string(), "service".to_string())]
                .into_iter()
                .collect(),
        );
        Arc::make_mut(&mut cluster.services[0])
            .spec
            .as_mut()
            .unwrap()
            .cluster_ip = Some("10.96.0.10".to_string());

        let Converted { hosts, .. } = hosts(&cluster, "proksi");

        // The service's own address and port, balanced by Kubernetes
        assert_eq!(upstreams(&hosts[0], 0), vec!["10.96.0.10:80"]);
    }

    fn with_annotations(
        mut cluster: Cluster,
        annotations: &[(&str, &str)],
    ) -> Cluster {
        Arc::make_mut(&mut cluster.ingresses[0]).metadata.annotations = Some(
            annotations
                .iter()
                .map(|(name, value)| (name.to_string(), value.to_string()))
                .collect(),
        );
        cluster
    }

    fn two_paths() -> Cluster {
        cluster(vec![ingress(
            "web",
            "2026-01-01T00:00:00Z",
            json!({
                "ingressClassName": "proksi",
                "rules": [rule(Some("a.test"), json!([
                    path(Some("/"), "Prefix", "web", json!({ "number": 80 })),
                    path(Some("/api"), "Prefix", "web", json!({ "number": 80 })),
                ]))],
            }),
        )])
    }

    #[test]
    fn test_blocks_for_every_path() {
        let cluster = with_annotations(
            two_paths(),
            &[(
                "proksi.ingress/blocks",
                "rate_limit {\n  requests = 10\n  per = \"1m\"\n}\nheaders {\n  response {\n    add = { \"x-from\" = \"ingress\" }\n  }\n}\n",
            )],
        );

        let Converted { hosts, invalid, .. } = hosts(&cluster, "proksi");

        assert_eq!(invalid, vec![]);
        for location in &hosts[0].locations {
            let plugins = location.plugins.as_ref().unwrap();
            assert_eq!(plugins[0].name, "rate_limit", "{}", location.path);
            assert_eq!(plugins[0].config.as_ref().unwrap()["requests"], 10);
            let headers = location.headers.as_ref().unwrap().add.as_ref().unwrap();
            assert_eq!(headers[0].name, "x-from");
        }
    }

    #[test]
    fn test_auth_settings_from_a_secret() {
        let mut cluster = with_annotations(
            two_paths(),
            &[
                ("proksi.ingress/blocks", "basic_auth {}"),
                ("proksi.ingress/auth-secret", "web-auth"),
            ],
        );
        cluster.secrets.push(Arc::new(object(json!({
            "metadata": {
                "name": "web-auth",
                "namespace": "default",
                "labels": { "proksi.ingress/auth": "true" },
            },
            // "admin" and "secret"
            "data": { "user": "YWRtaW4=", "pass": "c2VjcmV0" },
        }))));

        let Converted { hosts, invalid, .. } = hosts(&cluster, "proksi");

        assert_eq!(invalid, vec![]);
        let config = hosts[0].locations[0].plugins.as_ref().unwrap()[0]
            .config
            .clone()
            .unwrap();
        assert_eq!(config["user"], "admin");
        assert_eq!(config["pass"], "secret");
    }

    #[test]
    fn test_invalid_blocks() {
        for (annotations, reason) in [
            (
                vec![("proksi.ingress/blocks", "rate_limit {")],
                "invalid proksi.ingress/blocks: ",
            ),
            (
                vec![("proksi.ingress/blocks", "upstream \"10.0.0.1:80\" {}")],
                "proksi.ingress/blocks can't have upstreams: they come from the Ingress' backends",
            ),
            (
                vec![
                    ("proksi.ingress/blocks", "basic_auth {}"),
                    ("proksi.ingress/auth-secret", "missing"),
                ],
                "auth secret default/missing not found (it needs the label proksi.ingress/auth)",
            ),
        ] {
            let cluster = with_annotations(two_paths(), &annotations);

            let Converted { hosts, invalid, .. } = hosts(&cluster, "proksi");

            // The Ingress isn't served
            assert_eq!(hosts, vec![], "{annotations:?}");
            assert_eq!(invalid.len(), 1);
            assert!(invalid[0].reason.starts_with(reason), "{}", invalid[0].reason);
        }
    }
}
