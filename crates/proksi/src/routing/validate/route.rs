use crate::config::Route;

use super::common::{validate_headers, validate_upstreams};

/// Validates a route, whatever its source.
/// A route without upstreams is valid (requests get a `503`).
pub fn validate_route(route: &Route) -> Result<(), String> {
    if route.host.trim().is_empty() {
        return Err("host cannot be empty".to_string());
    }
    validate_upstreams(&route.upstreams)?;
    validate_headers(route.headers.as_ref())
}

#[cfg(test)]
mod tests {
    use std::borrow::Cow;

    use crate::config::{RouteHeader, RouteHeaderAdd};
    use crate::routing::test_utils::route;

    use super::validate_route;

    #[test]
    fn test_valid_route() {
        assert!(validate_route(&route("example.com", "10.0.0.1")).is_ok());
    }

    #[test]
    fn test_route_without_upstreams_is_valid() {
        let mut route = route("example.com", "10.0.0.1");
        route.upstreams.clear();
        assert!(validate_route(&route).is_ok());
    }

    #[test]
    fn test_route_with_empty_host() {
        let err = validate_route(&route("", "10.0.0.1")).unwrap_err();
        assert_eq!(err, "host cannot be empty");
    }

    #[test]
    fn test_route_with_empty_upstream_ip() {
        let err = validate_route(&route("example.com", "")).unwrap_err();
        assert_eq!(err, "upstreams[0].ip cannot be empty");
    }

    #[test]
    fn test_route_with_upstream_port_zero() {
        let mut route = route("example.com", "10.0.0.1");
        route.upstreams[0].port = 0;
        let err = validate_route(&route).unwrap_err();
        assert_eq!(err, "upstreams[0].port must be greater than 0");
    }

    #[test]
    fn test_route_with_invalid_header() {
        let mut route = route("example.com", "10.0.0.1");
        route.headers = Some(RouteHeader {
            add: Some(vec![RouteHeaderAdd {
                name: Cow::Borrowed("invalid header"),
                value: Cow::Borrowed("value"),
            }]),
            remove: None,
        });
        let err = validate_route(&route).unwrap_err();
        assert_eq!(
            err,
            "headers.add[0] is not a valid header: \"invalid header\""
        );
    }
}
