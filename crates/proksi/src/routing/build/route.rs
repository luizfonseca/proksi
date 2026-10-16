use std::{str::FromStr, time::Duration};

use http::{HeaderName, HeaderValue};
use pingora::lb::{health_check::TcpHealthCheck, selection::RoundRobin, LoadBalancer};

use anyhow::anyhow;
use openssl::{pkey::PKey, x509::X509};

use crate::{
    config::Route,
    stores::{self, certificates::Certificate, routes::RouteStoreContainer},
};

/// Loads the certificate of a route that has one in files (`ssl.path`) into the store
pub async fn load_route_certificate(route: &Route) -> Result<(), anyhow::Error> {
    let Some(ssl_path) = route.ssl.as_ref().and_then(|v| v.path.as_ref()) else {
        return Ok(());
    };

    let key = std::fs::read(&ssl_path.key)
        .map_err(anyhow::Error::from)
        .and_then(|key| Ok(PKey::private_key_from_pem(&key)?))
        .map_err(|err| anyhow!("failed to load private key from {:?}: {err}", ssl_path.key))?;
    let leaf = std::fs::read(&ssl_path.pem)
        .map_err(anyhow::Error::from)
        .and_then(|pem| Ok(X509::from_pem(&pem)?))
        .map_err(|err| anyhow!("failed to load certificate from {:?}: {err}", ssl_path.pem))?;

    stores::global::get_store()
        .set_certificate(
            &route.host.to_lowercase(),
            Certificate {
                key,
                leaf,
                chain: None,
            },
        )
        .await
        .map_err(|err| anyhow!("failed to store certificate for {}: {err}", route.host))
}

/// Plugins that can be enabled for a route
const SUPPORTED_PLUGINS: [&str; 3] = ["oauth2", "request_id", "basic_auth"];

/// Builds what the proxy needs to serve a route (load balancer, matchers, headers...)
pub fn build_route_container(route: &Route) -> Option<RouteStoreContainer> {
    let upstream_addrs = route
        .upstreams
        .iter()
        .map(|u| format!("{}:{}", u.ip, u.port))
        .collect::<Vec<String>>();

    let mut load_balancer = match LoadBalancer::<RoundRobin>::try_from_iter(upstream_addrs) {
        Ok(load_balancer) => load_balancer,
        Err(err) => {
            tracing::warn!(
                "could not create upstreams for host {}: {err}, upstreams {:?}",
                route.host,
                route.upstreams
            );
            return None;
        }
    };

    // TODO: support defining health checks in the configuration file
    load_balancer.set_health_check(TcpHealthCheck::new());
    load_balancer.health_check_frequency = Some(Duration::from_secs(15));

    let mut container = RouteStoreContainer::new(load_balancer);
    container.self_signed_certificate = route
        .ssl_certificate
        .as_ref()
        .and_then(|ssl| ssl.self_signed_on_failure)
        .unwrap_or(false);
    container.set_upstreams(route.upstreams.clone());
    container.cache.clone_from(&route.cache);

    if let Some(headers) = &route.headers {
        container.host_header_add = headers
            .add
            .iter()
            .flatten()
            .filter_map(|header| {
                let name = HeaderName::from_str(&header.name).ok()?;
                let value = HeaderValue::from_str(&header.value).ok()?;
                Some((name, value))
            })
            .collect();
        container.host_header_remove = headers
            .remove
            .iter()
            .flatten()
            .map(|header| header.name.to_string())
            .collect();
    }

    for plugin in route.plugins.iter().flatten() {
        if SUPPORTED_PLUGINS.contains(&plugin.name.as_ref()) {
            container
                .plugins
                .insert(plugin.name.to_string(), plugin.clone());
        }
    }

    // TODO: enable matchers for upstreams for true load balancing based on path
    if let Some(patterns) = route
        .match_with
        .as_ref()
        .and_then(|m| m.path.as_ref())
        .map(|p| &p.patterns)
    {
        container.path_matcher.with_pattern(patterns);
    }

    Some(container)
}

#[cfg(test)]
mod tests {
    use crate::routing::test_utils::route;

    use super::build_route_container;

    #[test]
    fn test_build_route_container() {
        let route = serde_json::from_value(serde_json::json!({
            "host": "example.com",
            "upstreams": [{ "ip": "10.0.0.1", "port": 3000 }, { "ip": "10.0.0.2", "port": 3000 }],
            "headers": {
                "add": [{ "name": "x-added", "value": "yes" }],
                "remove": [{ "name": "server" }],
            },
            "match_with": { "path": { "patterns": ["/api/*"] } },
            "plugins": [{ "name": "request_id" }, { "name": "unknown" }],
            "ssl_certificate": { "self_signed_on_failure": true },
        }))
        .unwrap();

        let container = build_route_container(&route).unwrap();

        assert_eq!(container.load_balancer.backends().get_backend().len(), 2);
        assert_eq!(container.upstreams.len(), 2);
        assert_eq!(container.host_header_add.len(), 1);
        assert_eq!(container.host_header_add[0].0.as_str(), "x-added");
        assert_eq!(container.host_header_remove, vec!["server".to_string()]);
        assert!(container.path_matcher.pattern.is_some());
        assert_eq!(
            container.plugins.keys().collect::<Vec<_>>(),
            vec!["request_id"]
        );
        assert!(container.self_signed_certificate);
    }

    #[tokio::test]
    async fn test_load_route_certificate() {
        let dir = tempfile::tempdir().unwrap();
        let key =
            openssl::pkey::PKey::from_rsa(openssl::rsa::Rsa::generate(2048).unwrap()).unwrap();
        let mut cert = openssl::x509::X509::builder().unwrap();
        cert.set_pubkey(&key).unwrap();
        cert.set_not_before(&openssl::asn1::Asn1Time::days_from_now(0).unwrap())
            .unwrap();
        cert.set_not_after(&openssl::asn1::Asn1Time::days_from_now(1).unwrap())
            .unwrap();
        cert.sign(&key, openssl::hash::MessageDigest::sha256())
            .unwrap();
        let cert = cert.build();
        std::fs::write(
            dir.path().join("key.pem"),
            key.private_key_to_pem_pkcs8().unwrap(),
        )
        .unwrap();
        std::fs::write(dir.path().join("cert.pem"), cert.to_pem().unwrap()).unwrap();

        let route = serde_json::from_value(serde_json::json!({
            "host": "Certificate.Test",
            "upstreams": [],
            "ssl": { "path": {
                "key": dir.path().join("key.pem"),
                "pem": dir.path().join("cert.pem"),
            } },
        }))
        .unwrap();

        super::load_route_certificate(&route).await.unwrap();

        let stored = crate::stores::global::get_store()
            .get_certificate("certificate.test")
            .await
            .unwrap();
        assert_eq!(stored.leaf.to_pem().unwrap(), cert.to_pem().unwrap());
    }

    #[test]
    fn test_build_route_container_without_upstreams() {
        let mut route = route("example.com", "10.0.0.1");
        route.upstreams.clear();

        let container = build_route_container(&route).unwrap();

        assert!(container.load_balancer.backends().get_backend().is_empty());
    }
}
