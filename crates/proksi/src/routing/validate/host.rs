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
        validate_splits(location).map_err(in_location)?;
        validate_mirror(location).map_err(in_location)?;
        validate_headers(location.headers.as_ref()).map_err(in_location)?;
        let blocks = Blocks::build(location).map_err(in_location)?;
        if !location.upstreams.is_empty()
            && blocks
                .iter()
                .any(|block| block.kind == crate::blocks::Kind::StaticFiles)
        {
            return Err(in_location(
                "static_files answers its requests, so it has no upstreams".to_string(),
            ));
        }
        if let Some(rewrite) = &location.rewrite {
            rewrite.compile().map_err(in_location)?;
        }
        crate::rules::Rules::compile(&location.rules).map_err(in_location)?;
    }
    crate::rules::Rules::compile(&host.rules)?;

    Ok(())
}

/// Traffic splits: each with upstreams, taking 100% of the requests at most together, the rest
/// going to the location's own upstreams
fn validate_splits(location: &crate::config::host::Location) -> Result<(), String> {
    // Rules choose splits of their location
    for rule in &location.rules {
        let chosen = rule
            .0
            .pointer("/then/traffic_split")
            .and_then(|name| name.as_str());
        if let Some(name) = chosen {
            if !location.splits.iter().any(|split| split.name == name) {
                let rule_name = rule
                    .0
                    .get("name")
                    .and_then(|name| name.as_str())
                    .unwrap_or("");
                return Err(format!(
                    "rule \"{rule_name}\": no traffic_split \"{name}\" in the location"
                ));
            }
        }
    }
    if location.splits.is_empty() {
        return Ok(());
    }
    if location.upstreams.is_empty() {
        return Err("traffic splits need the location's own upstreams too".to_string());
    }
    let mut total = 0u32;
    for split in &location.splits {
        let in_split = |err: String| format!("traffic_split \"{}\": {err}", split.name);
        if split.percent > 100 {
            return Err(in_split("percent is 0 to 100".to_string()));
        }
        if split.upstreams.is_empty() {
            return Err(in_split("needs an upstream".to_string()));
        }
        validate_upstreams(&split.upstreams).map_err(in_split)?;
        if let Some(cookie) = &split.cookie {
            let token = !cookie.is_empty()
                && cookie
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || b"!#$%&'*+-.^_`|~".contains(&byte));
            if !token {
                return Err(in_split(format!(
                    "cookie \"{cookie}\" is not a cookie name"
                )));
            }
        }
        total += u32::from(split.percent);
    }
    if total > 100 {
        return Err(format!(
            "the traffic splits take {total}% of the requests, more than 100%"
        ));
    }
    Ok(())
}

/// A mirror: of a location proxying its requests, with upstreams of its own
fn validate_mirror(location: &crate::config::host::Location) -> Result<(), String> {
    // Rules copy requests to their location's mirror
    for rule in &location.rules {
        let mirrors = rule
            .0
            .pointer("/then/mirror")
            .and_then(serde_json::Value::as_bool);
        if mirrors == Some(true) && location.mirror.is_none() {
            let rule_name = rule
                .0
                .get("name")
                .and_then(|name| name.as_str())
                .unwrap_or("");
            return Err(format!("rule \"{rule_name}\": no mirror in the location"));
        }
    }
    let Some(mirror) = &location.mirror else {
        return Ok(());
    };
    let in_mirror = |err: &str| format!("mirror: {err}");
    if location.upstreams.is_empty() {
        return Err(in_mirror(
            "copies the requests proxied to the location's upstreams, and it has none",
        ));
    }
    if mirror.upstreams.is_empty() {
        return Err(in_mirror("needs an upstream"));
    }
    if mirror.percent > 100 {
        return Err(in_mirror("percent is 0 to 100"));
    }
    if mirror.max_in_flight == 0 {
        return Err(in_mirror("max_in_flight is 1 or more"));
    }
    if mirror.timeout_secs == 0 {
        return Err(in_mirror("timeout_secs is 1 or more"));
    }
    validate_upstreams(&mirror.upstreams).map_err(|err| in_mirror(&err))
}

/// `*` serves requests for any other name, over plain HTTP (certificates are issued per name).
/// `*.example.com` serves names one label under `example.com` (ADR 0009).
fn validate_wildcard(host: &Host) -> Result<(), String> {
    if host.name == "*" {
        if host.tls.enabled {
            return Err(
                "host \"*\" needs tls = false (certificates are issued per name)".to_string(),
            );
        }
        return Ok(());
    }
    if !host.name.contains('*') {
        return Ok(());
    }
    let parent = host.name.strip_prefix("*.").unwrap_or_default();
    let labels = parent.split('.').collect::<Vec<_>>();
    if labels.len() < 2
        || labels
            .iter()
            .any(|label| label.is_empty() || label.contains('*'))
    {
        return Err("a wildcard host is \"*\" or \"*.\" followed by a name of two labels or more (\"*.example.com\")".to_string());
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
    fn test_static_files_answer_without_upstreams() {
        let directory = tempfile::tempdir().unwrap();
        let mut files = host("a.test", "10.0.0.1");
        files.locations[0].plugins = Some(vec![crate::config::RoutePlugin {
            name: "static_files".into(),
            config: Some(std::collections::HashMap::from([(
                "directory".into(),
                serde_json::json!(directory.path()),
            )])),
        }]);
        assert_eq!(
            validate_host(&files),
            Err(
                "location \"/*\": static_files answers its requests, so it has no upstreams"
                    .to_string()
            )
        );

        files.locations[0].upstreams.clear();
        assert_eq!(validate_host(&files), Ok(()));
    }

    #[test]
    fn test_traffic_splits() {
        let split = |name: &str, percent: u8| {
            serde_json::from_value::<crate::config::host::TrafficSplit>(serde_json::json!({
                "name": name,
                "percent": percent,
                "upstreams": [{ "ip": "10.0.0.2", "port": 80 }],
            }))
            .unwrap()
        };
        let with = |splits: Vec<crate::config::host::TrafficSplit>| {
            let mut host = host("a.test", "10.0.0.1");
            host.locations[0].splits = splits;
            validate_host(&host)
        };

        assert_eq!(with(vec![split("v2", 10), split("v3", 90)]), Ok(()));
        assert_eq!(
            with(vec![split("v2", 60), split("v3", 50)]),
            Err(
                "location \"/*\": the traffic splits take 110% of the requests, more than 100%"
                    .to_string()
            )
        );
        assert_eq!(
            with(vec![split("v2", 101)]),
            Err("location \"/*\": traffic_split \"v2\": percent is 0 to 100".to_string())
        );
        let mut empty = split("v2", 10);
        empty.upstreams.clear();
        assert_eq!(
            with(vec![empty]),
            Err("location \"/*\": traffic_split \"v2\": needs an upstream".to_string())
        );
        let mut bad_cookie = split("v2", 10);
        bad_cookie.cookie = Some("a b".to_string());
        assert_eq!(
            with(vec![bad_cookie]),
            Err(
                "location \"/*\": traffic_split \"v2\": cookie \"a b\" is not a cookie name"
                    .to_string()
            )
        );

        // Rules choose splits of their location
        let mut chooses = host("a.test", "10.0.0.1");
        chooses.locations[0].splits = vec![split("v2", 10)];
        chooses.locations[0].rules = vec![crate::rules::Rule(serde_json::json!({
            "name": "beta", "and": { "method": { "eq": "GET" } }, "then": { "traffic_split": "v3" },
        }))];
        assert_eq!(
            validate_host(&chooses),
            Err(
                "location \"/*\": rule \"beta\": no traffic_split \"v3\" in the location"
                    .to_string()
            )
        );

        // The requests the splits don't take go to the location's upstreams
        let mut host = host("a.test", "10.0.0.1");
        host.locations[0].upstreams.clear();
        host.locations[0].splits = vec![split("v2", 10)];
        assert_eq!(
            validate_host(&host),
            Err(
                "location \"/*\": traffic splits need the location's own upstreams too".to_string()
            )
        );
    }

    #[test]
    fn test_mirrors() {
        let mirror = || {
            serde_json::from_value::<crate::config::host::Mirror>(serde_json::json!({
                "upstreams": [{ "ip": "10.0.0.9", "port": 80 }],
            }))
            .unwrap()
        };
        let with = |change: &dyn Fn(&mut crate::config::host::Mirror)| {
            let mut host = host("a.test", "10.0.0.1");
            let mut mirror = mirror();
            change(&mut mirror);
            host.locations[0].mirror = Some(mirror);
            validate_host(&host)
        };

        assert_eq!(with(&|_| {}), Ok(()));
        let error = |message: &str| Err(format!("location \"/*\": mirror: {message}"));
        assert_eq!(
            with(&|mirror| mirror.upstreams.clear()),
            error("needs an upstream")
        );
        assert_eq!(
            with(&|mirror| mirror.percent = 101),
            error("percent is 0 to 100")
        );
        assert_eq!(
            with(&|mirror| mirror.max_in_flight = 0),
            error("max_in_flight is 1 or more")
        );
        assert_eq!(
            with(&|mirror| mirror.timeout_secs = 0),
            error("timeout_secs is 1 or more")
        );

        // Only proxied requests are copied
        let mut files = host("a.test", "10.0.0.1");
        files.locations[0].upstreams.clear();
        files.locations[0].mirror = Some(mirror());
        assert_eq!(
            validate_host(&files),
            error("copies the requests proxied to the location's upstreams, and it has none")
        );

        // Rules copy requests to their location's mirror
        let mut rules = host("a.test", "10.0.0.1");
        rules.locations[0].rules = vec![crate::rules::Rule(serde_json::json!({
            "name": "checkout", "and": { "method": { "eq": "POST" } }, "then": { "mirror": true },
        }))];
        assert_eq!(
            validate_host(&rules),
            Err("location \"/*\": rule \"checkout\": no mirror in the location".to_string())
        );
        rules.locations[0].mirror = Some(mirror());
        assert_eq!(validate_host(&rules), Ok(()));
    }

    #[test]
    fn test_wildcard_hosts() {
        // One label, under a name of two labels or more, with or without TLS
        assert!(validate_host(&host("*.example.com", "10.0.0.1")).is_ok());
        assert!(validate_host(&host("*.a.example.com", "10.0.0.1")).is_ok());

        let refused = "a wildcard host is \"*\" or \"*.\" followed by a name of two labels or more (\"*.example.com\")";
        for name in [
            "*.com",
            "*example.com",
            "a.*.example.com",
            "*.*.example.com",
            "**.example.com",
            "*.example.com.",
            "*..example.com",
        ] {
            let mut wildcard = host(name, "10.0.0.1");
            wildcard.tls.enabled = false;
            assert_eq!(validate_host(&wildcard), Err(refused.to_string()), "{name}");
        }
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
