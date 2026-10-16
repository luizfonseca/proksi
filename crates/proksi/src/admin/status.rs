//! `/api/v1/status`, `/api/v1/hosts` and `/api/v1/certificates` (ADR 0004)

use std::time::Instant;

use serde_json::{json, Value};

use crate::stores::store_trait::Store;

/// What the admin API reports about this instance, set when it starts
pub struct Instance {
    pub started: Instant,
    pub store_type: String,
    /// The addresses proksi listens on
    pub listeners: Value,
}

/// Version, replica, listeners, the store's revision and its replicas, and route sources
pub async fn status(store: &dyn Store, instance: &Instance) -> Value {
    let routing = crate::routing::reconciler::routing_status();
    let mut by_source = serde_json::Map::new();
    for (_, source) in &routing.routes {
        let name = format!("{source:?}").to_lowercase();
        let count = by_source.get(&name).and_then(Value::as_u64).unwrap_or_default();
        by_source.insert(name, json!(count + 1));
    }

    let (revision, error) = match store.revision().await {
        Ok(revision) => (json!(revision), Value::Null),
        Err(err) => (Value::Null, json!(err.to_string())),
    };
    let replicas = store.replicas().await.unwrap_or_default();

    json!({
        "version": env!("CARGO_PKG_VERSION"),
        "replica": crate::stores::replica_id(),
        "hostname": crate::stores::hostname(),
        "uptime_seconds": instance.started.elapsed().as_secs(),
        "listeners": instance.listeners,
        "store": {
            "type": instance.store_type,
            "revision": revision,
            "applied_revision": routing.store_revision,
            "replicas": replicas,
            "error": error,
        },
        "routes": {
            "by_source": by_source,
            "conflicts": routing.conflicts,
            "invalid": routing.invalid,
        },
    })
}

/// The hosts being served: their source, locations, blocks and upstreams (with their health).
/// Block settings aren't included: they can hold secrets.
pub fn hosts() -> Value {
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
                    let health = location.upstream_health();
                    let upstreams = location
                        .upstreams
                        .iter()
                        .zip(health)
                        .map(|(upstream, healthy)| {
                            json!({
                                "upstream": format!("{}:{}", upstream.ip, upstream.port),
                                "weight": upstream.weight.unwrap_or(1),
                                "healthy": healthy,
                            })
                        })
                        .collect::<Vec<_>>();
                    json!({
                        "location": location.path,
                        "blocks": location.blocks.iter().map(|block| block.kind.name()).collect::<Vec<_>>(),
                        "upstreams": upstreams,
                    })
                })
                .collect::<Vec<_>>();
            json!({
                "host": name,
                "source": source,
                "tls": host.tls_enabled,
                "listen": host.listen,
                "locations": locations,
            })
        })
        .collect::<Vec<_>>();
    hosts.sort_by(|a, b| a["host"].as_str().cmp(&b["host"].as_str()));
    Value::Array(hosts)
}

/// The certificates in the store: when they expire, who issued them, and who holds their lease
pub async fn certificates(store: &dyn Store) -> Value {
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

        list.push(json!({
            "domain": domain,
            "issuer": issuer,
            "self_signed": self_signed,
            "expires_at": crate::metrics::expiry_timestamp(&certificate),
            "days_left": days_left,
            "lease_holder": lease_holder,
        }));
    }
    list.sort_by(|a, b| a["domain"].as_str().cmp(&b["domain"].as_str()));
    Value::Array(list)
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
        }
    }

    #[tokio::test]
    async fn test_status() {
        let store = MemoryStore::new();

        let status = super::status(&store, &instance()).await;

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

        let hosts = super::hosts();
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
                "upstreams": [{ "upstream": "10.0.0.7:3000", "weight": 1, "healthy": true }],
            }])
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

        let certificates = super::certificates(&store).await;

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
