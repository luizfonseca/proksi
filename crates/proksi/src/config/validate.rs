use std::collections::HashSet;

use anyhow::anyhow;

use super::Config;
use crate::routing::validate::{validate_host, validate_route};

/// given a Config struct, validate the values to ensure
/// That we program won't panic when we try to use them
pub fn check_config(config: &Config) -> Result<(), anyhow::Error> {
    // Validate if worker threads is greater than 0
    if config.worker_threads.is_some_and(|v| v == 0) {
        return Err(anyhow!("Worker threads must be greater than 0"));
    }

    if config.store.sync_interval_secs == 0 {
        return Err(anyhow!("store.sync_interval_secs must be greater than 0"));
    }

    // Validate that the docker interval secs is greater than 0
    if config.docker.interval_secs.is_some_and(|v| v == 0) {
        return Err(anyhow!("docker.interval_secs must be greater than 0"));
    }

    // validate that the lets encrypt email does not contain @example or is empty, but only if lets_encrypt is enabled
    if config.lets_encrypt.enabled.unwrap_or(false)
        && (config.lets_encrypt.email.contains("@example") || config.lets_encrypt.email.is_empty())
    {
        return Err(anyhow!(
            "lets_encrypt.email cannot be empty or an email from @example.com (the default value)"
        ));
    }

    // Validate that the lets_encrypt pathbuf is not an empty string
    if config.paths.lets_encrypt.as_os_str() == "" {
        return Err(anyhow!("paths.lets_encrypt cannot be empty"));
    }

    // Validate the routes
    for (route_index, route) in config.routes.iter().enumerate() {
        validate_route(route).map_err(|err| anyhow!("routes[{route_index}]: {err}"))?;
    }

    check_hosts(config)?;
    check_without_https(config)
}

/// Without the HTTPS port, every host in the configuration file must be served over plain HTTP
fn check_without_https(config: &Config) -> Result<(), anyhow::Error> {
    if config.server.serves_https() {
        return Ok(());
    }
    if !config.routes.is_empty() {
        return Err(anyhow!(
            "routes = [...] always use TLS: use host blocks with tls = false when server.https_enabled is false"
        ));
    }
    if let Some(name) = config
        .hosts()
        .into_iter()
        .find(|host| host.tls.enabled)
        .map(|host| host.name)
    {
        return Err(anyhow!(
            "host \"{name}\" needs tls = false when server.https_enabled is false"
        ));
    }
    Ok(())
}

/// `host` blocks must be valid, and not define a host also in `routes`
fn check_hosts(config: &Config) -> Result<(), anyhow::Error> {
    for (name, block) in &config.host {
        let host = block
            .to_host(name)
            .map_err(|err| anyhow!("host \"{name}\": {err}"))?;
        validate_host(&host).map_err(|err| anyhow!("host \"{name}\": {err}"))?;
    }

    // Duplicates within `routes` keep being reported (and skipped) when merging
    let route_hosts = config
        .routes
        .iter()
        .map(|route| route.host.to_lowercase())
        .collect::<HashSet<_>>();
    if let Some(name) = config
        .host
        .keys()
        .map(|name| name.to_lowercase())
        .find(|name| route_hosts.contains(name))
    {
        return Err(anyhow!(
            "host \"{name}\" is defined both in a host block and in routes"
        ));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::config::Config;
    use crate::routing::test_utils::route;

    use super::check_config;

    #[test]
    fn test_check_config_store_sync_interval() {
        let mut config = Config::default();
        config.store.sync_interval_secs = 0;

        let err = check_config(&config).unwrap_err();

        assert_eq!(
            err.to_string(),
            "store.sync_interval_secs must be greater than 0"
        );
    }

    #[test]
    fn test_check_config_validates_routes() {
        let mut config = Config::default();
        let mut invalid = route("example.com", "10.0.0.1");
        invalid.upstreams[0].port = 0;
        config.routes = vec![route("valid.com", "10.0.0.1"), invalid];

        let err = check_config(&config).unwrap_err();

        assert_eq!(
            err.to_string(),
            "routes[1]: upstreams[0].port must be greater than 0"
        );
    }

    fn config_with_host_block(hcl: &str) -> Config {
        #[derive(serde::Deserialize)]
        struct Doc {
            host: std::collections::BTreeMap<String, crate::config::blocks::HostBlock>,
        }
        let doc =
            <crate::config::hcl::HclFormat as figment::providers::Format>::from_str::<Doc>(hcl)
                .unwrap();
        let mut config = Config::default();
        config.host = doc.host;
        config
    }

    #[test]
    fn test_invalid_host_block() {
        let config = config_with_host_block(
            r#"
host "a.test" {
  location "/v1" {
    upstream "10.0.0.1:3000" {}
  }
  location "/v1/*" {
    upstream "10.0.0.1:3000" {}
  }
}
"#,
        );

        assert_eq!(
            check_config(&config).unwrap_err().to_string(),
            "host \"a.test\": location \"/v1/*\": matches the same paths as location \"/v1\""
        );
    }

    #[test]
    fn test_host_block_with_invalid_upstream() {
        let config = config_with_host_block(
            "host \"a.test\" {\n  location \"/\" {\n    upstream \"10.0.0.1\" {}\n  }\n}\n",
        );

        assert_eq!(
            check_config(&config).unwrap_err().to_string(),
            "host \"a.test\": location \"/\": upstream \"10.0.0.1\" needs a port (host:port)"
        );
    }

    #[test]
    fn test_host_in_host_block_and_routes() {
        let mut config = config_with_host_block(
            "host \"A.test\" {\n  location \"/\" {\n    upstream \"10.0.0.1:3000\" {}\n  }\n}\n",
        );
        config.routes = vec![route("a.test", "10.0.0.2")];

        assert_eq!(
            check_config(&config).unwrap_err().to_string(),
            "host \"a.test\" is defined both in a host block and in routes"
        );
    }

    #[test]
    fn test_config_hosts_include_host_blocks_and_routes() {
        let mut config = config_with_host_block(
            "host \"a.test\" {\n  location \"/\" {\n    upstream \"10.0.0.1:3000\" {}\n  }\n}\n",
        );
        config.routes = vec![route("b.test", "10.0.0.2")];

        assert!(check_config(&config).is_ok());
        let names = config
            .hosts()
            .into_iter()
            .map(|h| h.name)
            .collect::<Vec<_>>();
        assert_eq!(names, vec!["a.test", "b.test"]);
    }

    #[test]
    fn test_without_https_hosts_need_tls_disabled() {
        let mut config = config_with_host_block(
            "host \"a.test\" {\n  location \"/\" {\n    upstream \"10.0.0.1:3000\" {}\n  }\n}\n",
        );
        config.server.https_enabled = Some(false);

        assert_eq!(
            check_config(&config).unwrap_err().to_string(),
            "host \"a.test\" needs tls = false when server.https_enabled is false"
        );
    }

    #[test]
    fn test_without_https_routes_cant_be_used() {
        let mut config = Config::default();
        config.server.https_enabled = Some(false);
        config.routes = vec![route("a.test", "10.0.0.1")];

        assert_eq!(
            check_config(&config).unwrap_err().to_string(),
            "routes = [...] always use TLS: use host blocks with tls = false when server.https_enabled is false"
        );
    }

    #[test]
    fn test_without_https_and_hosts_without_tls() {
        let mut config = config_with_host_block(
            "host \"*\" {\n  tls = false\n  location \"/\" {\n    upstream \"10.0.0.1:3000\" {}\n  }\n}\n",
        );
        config.server.https_enabled = Some(false);

        assert!(check_config(&config).is_ok());
        assert!(!config.server.serves_https());
    }

    #[test]
    fn test_https_is_enabled_by_default() {
        assert!(Config::default().server.serves_https());
    }
}
