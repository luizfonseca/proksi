use std::collections::HashMap;

use crate::{blocks::Blocks, config::host::Host};

use super::common::{validate_headers, validate_upstreams};

/// Validates a host and its locations, whatever its source.
/// A host without locations is valid (requests get a `404`).
pub fn validate_host(host: &Host) -> Result<(), String> {
    if host.name.trim().is_empty() {
        return Err("host cannot be empty".to_string());
    }
    validate_wildcard(host)?;
    validate_listen(&host.listen)?;

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
        Blocks::build(location).map_err(in_location)?;
        if let Some(rewrite) = &location.rewrite {
            rewrite.compile().map_err(in_location)?;
        }
    }

    Ok(())
}

/// `*` serves requests for any other name, over plain HTTP (certificates are issued per name)
fn validate_wildcard(host: &Host) -> Result<(), String> {
    if host.name == "*" {
        if host.tls.enabled {
            return Err(
                "host \"*\" needs tls = false (certificates are issued per name)".to_string(),
            );
        }
    } else if host.name.contains('*') {
        return Err("only \"*\" is supported as a wildcard host".to_string());
    }
    Ok(())
}

/// Listen addresses are bound when proksi starts, so they must be IP addresses
fn validate_listen(addresses: &[String]) -> Result<(), String> {
    match addresses
        .iter()
        .find(|address| address.parse::<std::net::SocketAddr>().is_err())
    {
        Some(address) => Err(format!(
            "listen \"{address}\" is not a valid address (ip:port)"
        )),
        None => Ok(()),
    }
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

    #[test]
    fn test_invalid_blocks_are_rejected() {
        let mut host = host("a.test", "10.0.0.1");
        host.locations[0].plugins = Some(vec![crate::config::RoutePlugin {
            name: "basic_auth".into(),
            config: None,
        }]);

        assert_eq!(
            validate_host(&host),
            Err("location \"/*\": basic_auth: `user` is required".to_string())
        );
    }

    #[test]
    fn test_upstream_weight_must_be_positive() {
        let mut host = host("a.test", "10.0.0.1");
        host.locations[0].upstreams[0].weight = Some(0);

        assert_eq!(
            validate_host(&host),
            Err("location \"/*\": upstreams[0].weight must be greater than 0".to_string())
        );
    }

    #[test]
    fn test_catch_all_host_needs_tls_disabled() {
        let mut catch_all = host("*", "10.0.0.1");
        assert_eq!(
            validate_host(&catch_all),
            Err("host \"*\" needs tls = false (certificates are issued per name)".to_string())
        );

        catch_all.tls.enabled = false;
        assert!(validate_host(&catch_all).is_ok());
    }

    #[test]
    fn test_only_catch_all_wildcard_is_supported() {
        let mut wildcard = host("*.example.com", "10.0.0.1");
        wildcard.tls.enabled = false;
        assert_eq!(
            validate_host(&wildcard),
            Err("only \"*\" is supported as a wildcard host".to_string())
        );
    }

    #[test]
    fn test_listen_addresses_must_be_ip_and_port() {
        let mut host = host("a.test", "10.0.0.1");
        host.listen = vec!["0.0.0.0:8443".to_string(), "localhost:80".to_string()];

        assert_eq!(
            validate_host(&host),
            Err("listen \"localhost:80\" is not a valid address (ip:port)".to_string())
        );
    }
}
