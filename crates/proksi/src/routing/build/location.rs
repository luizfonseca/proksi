use std::{
    collections::BTreeSet,
    future::Future,
    net::ToSocketAddrs,
    str::FromStr,
    task::{Context, Waker},
    time::Duration,
};

use http::{Extensions, HeaderName, HeaderValue};
use pingora::{
    lb::{
        discovery::Static, health_check::TcpHealthCheck, selection::RoundRobin, Backend, Backends,
        LoadBalancer,
    },
    protocols::l4::socket::SocketAddr,
};

use crate::{
    config::{host::Location, RouteUpstream},
    stores::routes::RouteStoreContainer,
};

/// Plugins that can be enabled for a location
pub(super) const SUPPORTED_PLUGINS: [&str; 3] = ["oauth2", "request_id", "basic_auth"];

/// Builds a location's container: load balancer (with health checks), headers, plugins, cache
pub(super) fn build_location_container(
    host: &str,
    location: &Location,
) -> Option<RouteStoreContainer> {
    let mut load_balancer = match load_balancer(&location.upstreams) {
        Ok(load_balancer) => load_balancer,
        Err(err) => {
            tracing::warn!(
                "could not create upstreams for host {host} (location {}): {err}, upstreams {:?}",
                location.path,
                location.upstreams
            );
            return None;
        }
    };

    // TODO: support defining health checks in the configuration file
    load_balancer.set_health_check(TcpHealthCheck::new());
    load_balancer.health_check_frequency = Some(Duration::from_secs(15));

    let mut container = RouteStoreContainer::new(load_balancer);
    container.set_upstreams(location.upstreams.clone());
    container.cache.clone_from(&location.cache);

    if let Some(headers) = &location.headers {
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

    for plugin in location.plugins.iter().flatten() {
        if SUPPORTED_PLUGINS.contains(&plugin.name.as_ref()) {
            container
                .plugins
                .insert(plugin.name.to_string(), plugin.clone());
        }
    }

    Some(container)
}

/// Round-robin between the upstreams, in proportion to their weight (1 by default).
/// Hostnames are resolved here, as with [`LoadBalancer::try_from_iter`].
fn load_balancer(upstreams: &[RouteUpstream]) -> std::io::Result<LoadBalancer<RoundRobin>> {
    let mut backends = BTreeSet::new();
    for upstream in upstreams {
        let weight = upstream
            .weight
            .and_then(|weight| usize::try_from(weight).ok())
            .unwrap_or(1);
        let addrs = format!("{}:{}", upstream.ip, upstream.port).to_socket_addrs()?;
        backends.extend(addrs.map(|addr| Backend {
            addr: SocketAddr::Inet(addr),
            weight,
            ext: Extensions::new(),
        }));
    }

    let load_balancer = LoadBalancer::from_backends(Backends::new(Static::new(backends)));
    // Loads the backends: static discovery completes on the first poll
    {
        let update = std::pin::pin!(load_balancer.update());
        let _ = update.poll(&mut Context::from_waker(Waker::noop()));
    }
    Ok(load_balancer)
}

#[cfg(test)]
mod tests {
    use crate::config::host::Location;

    use super::build_location_container;

    fn location() -> Location {
        serde_json::from_value(serde_json::json!({
            "path": "/api",
            "upstreams": [{ "ip": "10.0.0.1", "port": 3000 }, { "ip": "10.0.0.2", "port": 3000 }],
            "headers": {
                "add": [{ "name": "x-added", "value": "yes" }],
                "remove": [{ "name": "server" }],
            },
            "plugins": [{ "name": "request_id" }, { "name": "unknown" }],
        }))
        .unwrap()
    }

    #[test]
    fn test_build_location_container() {
        let container = build_location_container("example.com", &location()).unwrap();

        assert_eq!(container.load_balancer.backends().get_backend().len(), 2);
        assert_eq!(container.upstreams.len(), 2);
        assert_eq!(container.host_header_add.len(), 1);
        assert_eq!(container.host_header_add[0].0.as_str(), "x-added");
        assert_eq!(container.host_header_remove, vec!["server".to_string()]);
        assert_eq!(
            container.plugins.keys().collect::<Vec<_>>(),
            vec!["request_id"]
        );
    }

    #[test]
    fn test_upstreams_are_chosen_by_weight() {
        let mut location = location();
        location.upstreams[0].weight = Some(3);
        let container = build_location_container("example.com", &location).unwrap();

        let chosen = (0..400)
            .filter_map(|_| container.load_balancer.select(b"", 32))
            .filter(|backend| backend.addr.to_string() == "10.0.0.1:3000")
            .count();

        // 3 of every 4 requests
        assert_eq!(chosen, 300);
    }

    #[test]
    fn test_build_location_container_without_upstreams() {
        let mut location = location();
        location.upstreams.clear();

        let container = build_location_container("example.com", &location).unwrap();

        assert!(container.upstreams.is_empty());
        assert!(container.load_balancer.select(b"", 32).is_none());
    }
}
