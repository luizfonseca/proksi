use std::collections::HashMap;

use crate::config::host::Host;

use super::common::{validate_headers, validate_upstreams};

/// Validates a host and its locations, whatever its source.
/// A host without locations is valid (requests get a `404`).
pub fn validate_host(host: &Host) -> Result<(), String> {
    if host.name.trim().is_empty() {
        return Err("host cannot be empty".to_string());
    }

    // Pattern -> path of the location defining it
    let mut patterns: HashMap<String, &str> = HashMap::new();
    for location in &host.locations {
        let in_location = |err: String| format!("location \"{}\": {err}", location.path);

        validate_location_path(&location.path).map_err(in_location)?;
        for pattern in location.patterns() {
            if let Some(other) = patterns.insert(pattern, &location.path) {
                return Err(in_location(format!(
                    "matches the same paths as location \"{other}\""
                )));
            }
        }
        validate_upstreams(&location.upstreams).map_err(in_location)?;
        validate_headers(location.headers.as_ref()).map_err(in_location)?;
    }

    Ok(())
}

/// Locations are paths: nginx modifiers are rejected rather than read as literal paths
fn validate_location_path(path: &str) -> Result<(), String> {
    if path.starts_with('/') {
        return Ok(());
    }
    if ["=", "~", "^~"]
        .iter()
        .any(|modifier| path.starts_with(modifier))
    {
        return Err(
            "nginx modifiers (=, ~, ~*, ^~) are not supported, locations match the most specific path"
                .to_string(),
        );
    }
    Err("must start with /".to_string())
}

#[cfg(test)]
mod tests {
    use crate::{config::host::Host, routing::test_utils::host};

    use super::validate_host;

    fn with_location(path: &str) -> Host {
        let mut host = host("a.test", "10.0.0.1");
        host.locations[0].path = path.to_string();
        host.locations[0].exact = false;
        host
    }

    #[test]
    fn test_valid_host() {
        assert!(validate_host(&host("a.test", "10.0.0.1")).is_ok());
    }

    #[test]
    fn test_host_with_empty_name() {
        assert_eq!(
            validate_host(&host(" ", "10.0.0.1")),
            Err("host cannot be empty".to_string())
        );
    }

    #[test]
    fn test_location_with_nginx_modifier() {
        assert_eq!(
            validate_host(&with_location("= /x")),
            Err("location \"= /x\": nginx modifiers (=, ~, ~*, ^~) are not supported, locations match the most specific path".to_string())
        );
        assert!(validate_host(&with_location("~ ^/x")).is_err());
        assert!(validate_host(&with_location("~* ^/x")).is_err());
        assert!(validate_host(&with_location("^~ /x")).is_err());
    }

    #[test]
    fn test_location_without_leading_slash() {
        assert_eq!(
            validate_host(&with_location("api")),
            Err("location \"api\": must start with /".to_string())
        );
    }

    #[test]
    fn test_locations_matching_the_same_paths() {
        let mut host = with_location("/v1");
        let mut other = host.locations[0].clone();
        other.path = "/v1/*".to_string();
        host.locations.push(other);

        assert_eq!(
            validate_host(&host),
            Err("location \"/v1/*\": matches the same paths as location \"/v1\"".to_string())
        );
    }

    #[test]
    fn test_location_with_invalid_upstream() {
        let mut host = host("a.test", "10.0.0.1");
        host.locations[0].upstreams[0].port = 0;

        assert_eq!(
            validate_host(&host),
            Err("location \"/*\": upstreams[0].port must be greater than 0".to_string())
        );
    }
}
