//! `/api/v1/status`, `/api/v1/hosts` and `/api/v1/certificates` (ADR 0004)

use std::{collections::BTreeMap, time::Instant};

use serde::Serialize;
use serde_json::Value;

use crate::{
    routing::merge::{Conflict, InvalidRoute},
    stores::{store_trait::Store, stored_route::ReplicaStatus},
};

/// What the admin API reports about this instance, set when it starts
pub struct Instance {
    pub started: Instant,
    pub store_type: String,
    /// The addresses proksi listens on
    pub listeners: Value,
    /// The settings read at startup (none on the metrics-only listener)
    pub server: Option<super::server::Settings>,
}

/// `GET /api/v1/status`: version, replica, listeners, the store's revision and its replicas,
/// and route sources
#[derive(Debug, Serialize)]
pub struct Status {
    pub version: &'static str,
    pub replica: String,
    pub hostname: String,
    pub uptime_seconds: u64,
    pub listeners: Value,
    pub store: StoreStatus,
    pub routes: RoutesStatus,
}

#[derive(Debug, Serialize)]
pub struct StoreStatus {
    #[serde(rename = "type")]
    pub store_type: String,
    /// None if the store can't be reached (see `error`)
    pub revision: Option<u64>,
    /// The revision this instance serves
    pub applied_revision: Option<u64>,
    pub replicas: Vec<ReplicaStatus>,
    pub error: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct RoutesStatus {
    /// Hosts served, by source
    pub by_source: BTreeMap<String, u64>,
    pub conflicts: Vec<Conflict>,
    pub invalid: Vec<InvalidRoute>,
}

/// `GET /api/v1/hosts`: a host being served. Block settings aren't included: they can hold
/// secrets.
#[derive(Debug, Serialize)]
pub struct HostStatus {
    pub host: String,
    /// `file`, `docker`, `kubernetes` or `store`
    pub source: Option<String>,
    pub tls: bool,
    pub listen: Vec<String>,
    /// Rules of every request of the host, as written (ADR 0007), without secrets
    pub rules: Vec<Value>,
    pub locations: Vec<LocationStatus>,
}

#[derive(Debug, Serialize)]
pub struct LocationStatus {
    pub location: String,
    pub blocks: Vec<&'static str>,
    /// Rules of the location's requests, as written, without secrets
    pub rules: Vec<Value>,
    pub upstreams: Vec<UpstreamStatus>,
}

#[derive(Debug, Serialize)]
pub struct UpstreamStatus {
    /// As written (`ip:port` or `name:port`)
    pub upstream: String,
    /// What it came from: a docker label's upstream, or a Kubernetes service
    pub name: Option<String>,
    pub weight: i8,
    /// Whether one of its addresses is healthy
    pub healthy: bool,
    /// What it resolved to
    pub addresses: Vec<AddressStatus>,
}

#[derive(Debug, Serialize)]
pub struct AddressStatus {
    pub address: String,
    pub healthy: bool,
}

/// `GET /api/v1/certificates`: a certificate in the store
#[derive(Debug, Serialize)]
pub struct CertificateStatus {
    pub domain: String,
    pub issuer: Option<String>,
    pub self_signed: bool,
    /// Unix timestamp (seconds)
    pub expires_at: Option<i64>,
    pub days_left: Option<i32>,
    /// The instance renewing it
    pub lease_holder: Option<String>,
}

pub async fn status(store: &dyn Store, instance: &Instance) -> Status {
    let routing = crate::routing::reconciler::routing_status();
    let mut by_source = BTreeMap::new();
    for (_, source) in &routing.routes {
        *by_source
            .entry(format!("{source:?}").to_lowercase())
            .or_default() += 1;
    }
    let (revision, error) = match store.revision().await {
        Ok(revision) => (Some(revision), None),
        Err(err) => (None, Some(err.to_string())),
    };

    Status {
        version: env!("CARGO_PKG_VERSION"),
        replica: crate::stores::replica_id().to_string(),
        hostname: crate::stores::hostname(),
        uptime_seconds: instance.started.elapsed().as_secs(),
        listeners: instance.listeners.clone(),
        store: StoreStatus {
            store_type: instance.store_type.clone(),
            revision,
            applied_revision: routing.store_revision,
            replicas: store.replicas().await.unwrap_or_default(),
            error,
        },
        routes: RoutesStatus {
            by_source,
            conflicts: routing.conflicts.clone(),
            invalid: routing.invalid.clone(),
        },
    }
}

/// The hosts being served, by name
pub fn hosts() -> Vec<HostStatus> {
    let routing = crate::routing::reconciler::routing_status();
    let mut hosts = crate::stores::get_routes()
        .iter()
        .map(|(name, host)| {
            let source = routing
                .routes
                .iter()
                .find(|(route, _)| route == name)
                .map(|(_, source)| format!("{source:?}").to_lowercase());
            let locations = host
                .locations
                .iter()
                .map(|location| {
                    let upstreams = location
                        .upstreams
                        .iter()
                        .zip(location.upstream_health())
                        .zip(location.upstream_addresses())
                        .map(|((upstream, healthy), addresses)| UpstreamStatus {
                            upstream: format!("{}:{}", upstream.ip, upstream.port),
                            name: upstream.name.clone(),
                            weight: upstream.weight.unwrap_or(1),
                            healthy,
                            addresses: addresses
                                .into_iter()
                                .map(|(address, healthy)| AddressStatus { address, healthy })
                                .collect(),
                        })
                        .collect();
                    LocationStatus {
                        location: location.path.clone(),
                        blocks: location
                            .blocks
                            .iter()
                            .map(|block| block.kind.name())
                            .collect(),
                        rules: written_rules(location.rules.as_deref()),
                        upstreams,
                    }
                })
                .collect();
            HostStatus {
                host: name.clone(),
                source,
                tls: host.tls_enabled,
                listen: host.listen.iter().map(ToString::to_string).collect(),
                rules: written_rules(host.rules.as_deref()),
                locations,
            }
        })
        .collect::<Vec<_>>();
    hosts.sort_by(|a, b| a.host.cmp(&b.host));
    hosts
}

/// What's hidden of a rule
const HIDDEN: &str = "(hidden)";

/// Blocks whose settings are secrets (passwords, client secrets, auth endpoints' headers)
const SECRET_BLOCKS: [&str; 3] = ["basic_auth", "oauth2", "forward_auth"];

/// Rules as written, without what can be a secret: the values headers, cookies and query
/// parameters are compared with (API keys), the settings of authentication blocks, and the values of headers they add
fn written_rules(rules: Option<&crate::rules::Rules>) -> Vec<Value> {
    rules
        .map(|rules| rules.source().iter().map(|rule| redact(&rule.0)).collect())
        .unwrap_or_default()
}

fn redact(rule: &Value) -> Value {
    let Value::Object(rule) = rule else {
        return rule.clone();
    };
    rule.iter()
        .map(|(key, value)| {
            let value = match key.as_str() {
                "then" => redact_actions(value),
                "name" => value.clone(),
                _ => redact_condition(value),
            };
            (key.clone(), value)
        })
        .collect::<serde_json::Map<_, _>>()
        .into()
}

/// `header "name" { eq = "..." }`, `cookie` and `query`: names and `exists` stay, values are
/// hidden
fn redact_condition(condition: &Value) -> Value {
    match condition {
        Value::Object(fields) => fields
            .iter()
            .map(|(key, value)| {
                let value = if matches!(key.as_str(), "header" | "cookie" | "query") {
                    hide_values(value, &["exists"])
                } else {
                    redact_condition(value)
                };
                (key.clone(), value)
            })
            .collect::<serde_json::Map<_, _>>()
            .into(),
        Value::Array(conditions) => conditions.iter().map(redact_condition).collect(),
        other => other.clone(),
    }
}

fn redact_actions(actions: &Value) -> Value {
    let Value::Object(actions) = actions else {
        return actions.clone();
    };
    actions
        .iter()
        .map(|(block, settings)| {
            let settings = if SECRET_BLOCKS.contains(&block.as_str()) {
                Value::String(HIDDEN.to_string())
            } else if block == "headers" {
                hide_values(settings, &["remove"])
            } else {
                settings.clone()
            };
            (block.clone(), settings)
        })
        .collect::<serde_json::Map<_, _>>()
        .into()
}

/// Every value below `value` hidden, but keys, and the values of `keep`
fn hide_values(value: &Value, keep: &[&str]) -> Value {
    match value {
        Value::Object(fields) => fields
            .iter()
            .map(|(key, value)| {
                let value = if keep.contains(&key.as_str()) {
                    value.clone()
                } else {
                    hide_values(value, keep)
                };
                (key.clone(), value)
            })
            .collect::<serde_json::Map<_, _>>()
            .into(),
        Value::Array(values) => values
            .iter()
            .map(|value| hide_values(value, keep))
            .collect(),
        _ => Value::String(HIDDEN.to_string()),
    }
}

/// The certificates in the store, by domain
pub async fn certificates(store: &dyn Store) -> Vec<CertificateStatus> {
    let certificates = store
        .get_certificates()
        .await
        .iter()
        .map(|(domain, certificate)| (domain.clone(), certificate.clone()))
        .collect::<Vec<_>>();

    let mut list = vec![];
    for (domain, certificate) in certificates {
        let leaf = &certificate.leaf;
        let issuer = leaf
            .issuer_name()
            .entries_by_nid(openssl::nid::Nid::COMMONNAME)
            .next()
            .and_then(|entry| entry.data().as_utf8().ok())
            .map(|name| name.to_string());
        let self_signed = leaf
            .issuer_name()
            .try_cmp(leaf.subject_name())
            .is_ok_and(std::cmp::Ordering::is_eq);
        let days_left = openssl::asn1::Asn1Time::days_from_now(0)
            .and_then(|now| now.diff(leaf.not_after()))
            .map(|diff| diff.days)
            .ok();
        let lease_holder = store
            .lock_holder(&format!("acme:{domain}"))
            .await
            .ok()
            .flatten();

        list.push(CertificateStatus {
            expires_at: crate::metrics::expiry_timestamp(&certificate),
            domain,
            issuer,
            self_signed,
            days_left,
            lease_holder,
        });
    }
    list.sort_by(|a, b| a.domain.cmp(&b.domain));
    list
}

#[cfg(test)]
mod tests {
    use std::{sync::Arc, time::Duration};

    use serde_json::json;

    use crate::{
        routing::{build::build_host_container, test_utils::host},
        stores::{self, contract_tests::certificate, store_trait::Store, MemoryStore},
    };

    use super::Instance;

    fn instance() -> Instance {
        Instance {
            started: std::time::Instant::now() - Duration::from_secs(90),
            store_type: "memory".to_string(),
            listeners: json!({ "https": ["0.0.0.0:443"] }),
            server: None,
        }
    }

    #[tokio::test]
    async fn test_status() {
        let store = MemoryStore::new();

        let status = serde_json::to_value(super::status(&store, &instance()).await).unwrap();

        assert_eq!(status["version"], env!("CARGO_PKG_VERSION"));
        assert_eq!(status["replica"], stores::replica_id());
        assert_eq!(status["uptime_seconds"], 90);
        assert_eq!(status["listeners"]["https"][0], "0.0.0.0:443");
        assert_eq!(status["store"]["type"], "memory");
        assert_eq!(status["store"]["revision"], 0);
        assert_eq!(status["store"]["replicas"], json!([]));
        assert!(status["routes"]["conflicts"].is_array(), "{status}");
    }

    #[test]
    fn test_hosts() {
        let container = build_host_container(&host("admin-hosts.test", "10.0.0.7")).unwrap();
        stores::insert_route("admin-hosts.test".to_string(), Arc::new(container));

        let hosts = serde_json::to_value(super::hosts()).unwrap();
        stores::remove_route("admin-hosts.test");

        let host = hosts
            .as_array()
            .unwrap()
            .iter()
            .find(|host| host["host"] == "admin-hosts.test")
            .unwrap_or_else(|| panic!("{hosts}"));
        assert_eq!(host["tls"], true);
        assert_eq!(
            host["locations"],
            json!([{
                "location": "/*",
                "blocks": [],
                "rules": [],
                "upstreams": [{
                    "upstream": "10.0.0.7:3000",
                    "name": null,
                    "weight": 1,
                    "healthy": true,
                    "addresses": [{ "address": "10.0.0.7:3000", "healthy": true }],
                }],
            }])
        );
    }

    #[test]
    fn test_rules_without_secrets() {
        let rule = json!({
            "name": "partners",
            "and": {
                "method": { "eq": "POST" },
                "header": { "x-api-key": { "eq": "key-secret" }, "x-partner": { "exists": true } },
                "or": [
                    { "cookie": { "session": { "in": ["cookie-secret"] } } },
                    { "not": { "query": { "token": { "eq": "query-secret" } } } },
                ],
            },
            "then": {
                "rate_limit": { "requests": 10, "per": "1m" },
                "basic_auth": { "user": "admin", "pass": "auth-secret" },
                "headers": {
                    "request": { "add": { "authorization": "Bearer header-secret" }, "remove": ["x-debug"] },
                },
            },
        });

        let redacted = super::redact(&rule);

        let text = redacted.to_string();
        for secret in [
            "key-secret",
            "cookie-secret",
            "query-secret",
            "auth-secret",
            "header-secret",
        ] {
            assert!(!text.contains(secret), "{text}");
        }
        assert_eq!(redacted["name"], "partners");
        assert_eq!(redacted["and"]["method"], json!({ "eq": "POST" }));
        assert_eq!(
            redacted["and"]["header"]["x-partner"],
            json!({ "exists": true })
        );
        assert_eq!(
            redacted["and"]["header"]["x-api-key"],
            json!({ "eq": "(hidden)" })
        );
        assert_eq!(redacted["then"]["rate_limit"], rule["then"]["rate_limit"]);
        assert_eq!(redacted["then"]["basic_auth"], "(hidden)");
        assert_eq!(
            redacted["then"]["headers"]["request"],
            json!({ "add": { "authorization": "(hidden)" }, "remove": ["x-debug"] })
        );
    }

    #[tokio::test]
    async fn test_certificates() {
        let store = MemoryStore::new();
        store
            .set_certificate("cert.test", certificate("cert.test", 30))
            .await
            .unwrap();
        store
            .try_lock("acme:cert.test", "replica-1", Duration::from_secs(60))
            .await
            .unwrap();

        let certificates = serde_json::to_value(super::certificates(&store).await).unwrap();

        let certificate = &certificates[0];
        assert_eq!(certificate["domain"], "cert.test");
        assert_eq!(certificate["issuer"], "cert.test");
        assert_eq!(certificate["self_signed"], true);
        let days_left = certificate["days_left"].as_i64().unwrap();
        assert!((29..=30).contains(&days_left), "{certificate}");
        assert!(certificate["expires_at"].as_i64().unwrap() > 1_700_000_000);
        assert_eq!(certificate["lease_holder"], "replica-1");
    }
}
