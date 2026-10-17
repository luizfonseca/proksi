use std::time::Duration;

use pingora::lb::health_check::TcpHealthCheck;

use crate::{
    blocks::Blocks, config::host::Location, metrics::LocationMetrics,
    stores::routes::RouteStoreContainer,
};

use super::load_balancer::load_balancer;

/// Builds a location's container: load balancer (with health checks) and building blocks
pub(super) fn build_location_container(
    host: &str,
    location: &Location,
) -> Option<RouteStoreContainer> {
    let mut load_balancer = load_balancer(&location.upstreams);

    // TODO: support defining health checks in the configuration file
    load_balancer.set_health_check(TcpHealthCheck::new());
    load_balancer.health_check_frequency = Some(Duration::from_secs(15));

    let mut container = RouteStoreContainer::new(load_balancer);
    container.path.clone_from(&location.path);
    container.set_upstreams(location.upstreams.clone());
    container.cache.clone_from(&location.cache);
    container.timeouts = location.timeouts;
    container.rewrite.clone_from(&location.rewrite);
    let upstreams = location
        .upstreams
        .iter()
        .map(|upstream| format!("{}:{}", upstream.ip, upstream.port))
        .collect::<Vec<_>>();

    container.blocks = match Blocks::build(location) {
        Ok(blocks) => blocks,
        Err(err) => {
            tracing::warn!("invalid location {} of host {host}: {err}", location.path);
            return None;
        }
    };
    container.metrics = crate::metrics::enabled().then(|| LocationMetrics::with_blocks(
        host,
        &location.path,
        &upstreams,
        &container.blocks.answering(),
    ));

    Some(container)
}

#[cfg(test)]
mod tests {
    use crate::{blocks::Kind, config::host::Location};

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
        // The unknown plugin is ignored
        assert_eq!(
            container.blocks.iter().map(|b| b.kind).collect::<Vec<_>>(),
            vec![Kind::RequestId, Kind::Headers]
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
