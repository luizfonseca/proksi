use std::str::FromStr;

use http::{HeaderName, HeaderValue};

use crate::config::Route;

/// Validates a route, whatever its source.
/// A route without upstreams is valid (requests get a `503`).
pub fn validate_route(route: &Route) -> Result<(), String> {
    if route.host.trim().is_empty() {
        return Err("host cannot be empty".to_string());
    }

    for (index, upstream) in route.upstreams.iter().enumerate() {
        if upstream.ip.is_empty() {
            return Err(format!("upstreams[{index}].ip cannot be empty"));
        }
        if upstream.port == 0 {
            return Err(format!("upstreams[{index}].port must be greater than 0"));
        }
    }

    let headers_to_add = route.headers.as_ref().and_then(|h| h.add.as_ref());
    for (index, header) in headers_to_add.into_iter().flatten().enumerate() {
        if HeaderName::from_str(&header.name).is_err()
            || HeaderValue::from_str(&header.value).is_err()
        {
            return Err(format!(
                "headers.add[{index}] is not a valid header: {:?}",
                header.name
            ));
        }
    }

    Ok(())
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
