//! Schema version 1: a route (converted into a host when read)

use serde::Deserialize;

use crate::config::{host::Host, Route};

#[derive(Deserialize)]
struct StoredRouteV1 {
    route: Route,
}

/// Reads a stored route with schema version 1, as a host
pub(super) fn decode_v1(json: &str) -> Result<Host, serde_json::Error> {
    let stored: StoredRouteV1 = serde_json::from_str(json)?;
    Ok(Host::from_route(&stored.route))
}

#[cfg(test)]
mod tests {
    use super::decode_v1;

    #[test]
    fn test_decode_schema_version_1_route() {
        let json = serde_json::json!({
            "schema_version": 1,
            "route": {
                "host": "a.test",
                "upstreams": [{ "ip": "10.0.0.1", "port": 3000 }],
                "match_with": { "path": { "patterns": ["/api/*"] } },
            },
        })
        .to_string();

        let host = decode_v1(&json).unwrap();

        assert_eq!(host.name, "a.test");
        assert_eq!(host.locations[0].path, "/api/*");
        assert!(host.locations[0].exact);
        assert_eq!(host.locations[0].upstreams[0].ip, "10.0.0.1");
    }
}
