use std::collections::HashSet;

use anyhow::anyhow;

use super::{listeners::listeners, Config};
use crate::routing::validate::{validate_host, validate_route};

/// given a Config struct, validate the values to ensure
/// That we program won't panic when we try to use them
pub fn check_config(config: &Config) -> Result<(), anyhow::Error> {
    // Validate if worker threads is greater than 0
    if config.worker_threads.is_some_and(|v| v == 0) {
        return Err(anyhow!("Worker threads must be greater than 0"));
    }

    if config.kubernetes.enabled && !cfg!(feature = "kubernetes") {
        return Err(anyhow!(
            "proksi was built without the kubernetes ingress controller (cargo feature kubernetes)"
        ));
    }

    crate::proxy_server::client::TrustedProxies::parse(&config.server.trusted_proxies)
        .map_err(|err| anyhow!("server.{err}"))?;
    config
        .server
        .tls
        .validate()
        .map_err(|err| anyhow!("server.tls.{err}"))?;
    crate::rules::Rules::compile(&config.server.rule).map_err(|err| anyhow!("server: {err}"))?;

    let publish = config.kubernetes.publish_service.as_deref();
    if publish.is_some_and(|service| {
        service
            .split_once('/')
            .is_none_or(|(namespace, name)| namespace.is_empty() || name.is_empty())
    }) {
        return Err(anyhow!(
            "kubernetes.publish_service must be namespace/name, not {:?}",
            publish.unwrap_or_default()
        ));
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
    // A port can't serve both HTTP and HTTPS hosts
    listeners(config).map(|_| ()).map_err(|err| anyhow!(err))
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
    use figment::providers::Format;

    use crate::config::Config;
    use crate::routing::test_utils::route;

    use super::check_config;

    #[test]
    fn test_example_files_are_valid() {
        // In a jail: other tests set `PROKSI_*` variables, which loading reads
        figment::Jail::expect_with(|_| {
            example_files_are_valid();
            Ok(())
        });
    }

    fn example_files_are_valid() {
        for file in ["example.hcl", "example.yaml"] {
            let path = format!("{}/../../examples/{file}", env!("CARGO_MANIFEST_DIR"));
            let config = crate::config::load_for_test(&path)
                .unwrap_or_else(|err| panic!("{file} can't be loaded: {err}"));

            check_config(&config).unwrap_or_else(|err| panic!("{file} is invalid: {err}"));
            assert_eq!(config.hosts().len(), 3, "{file}");
            assert_eq!(
                config.tcp_routes().map(|routes| routes.len()),
                Ok(1),
                "{file}"
            );
        }
    }

    /// The top-level settings of a configuration file
    const SETTINGS: [&str; 15] = [
        "service_name",
        "worker_threads",
        "server",
        "logging",
        "lets_encrypt",
        "paths",
        "store",
        "cache",
        "auto_reload",
        "docker",
        "kubernetes",
        "admin",
        "host",
        "tcp",
        "routes",
    ];

    /// The configuration examples of the README: each is a whole configuration file, and loads.
    /// One starting with `# file: <path>` is a file another example imports.
    #[test]
    fn test_the_readme_examples_are_valid() {
        let readme = include_str!("../../../../README.md");
        figment::Jail::expect_with(|jail| {
            let mut count = 0;
            for (fence, file) in [("```hcl\n", "proksi.hcl"), ("```yaml\n", "proksi.yaml")] {
                for example in readme.split(fence).skip(1) {
                    let example = example.split("```").next().unwrap_or_default();
                    // A Docker Compose file, not proksi's
                    if example.starts_with("services:") {
                        continue;
                    }
                    for variable in example.split("env(\"").skip(1) {
                        let name = variable.split('"').next().unwrap_or_default();
                        jail.set_env(name, "secret");
                    }
                    let first = example.lines().next().unwrap_or_default();
                    if let Some(path) = first.strip_prefix("# file: ") {
                        if let Some((dir, _)) = path.rsplit_once('/') {
                            jail.create_dir(dir)?;
                        }
                        jail.create_file(path, example)?;
                        continue;
                    }

                    // A `location` outside a `host` would be ignored, not loaded
                    let value: Result<serde_json::Value, String> = if file.ends_with(".hcl") {
                        crate::config::hcl::HclFormat::from_str(example).map_err(|e| e.to_string())
                    } else {
                        figment::providers::Yaml::from_str(example).map_err(|e| e.to_string())
                    };
                    let value = value.unwrap_or_else(|err| panic!("{err}\n{example}"));
                    for key in value.as_object().into_iter().flat_map(|map| map.keys()) {
                        assert!(
                            SETTINGS.contains(&key.as_str()),
                            "`{key}` isn't a setting\n{example}"
                        );
                    }

                    jail.create_file(file, example)?;
                    let config = crate::config::load_for_test(file)
                        .unwrap_or_else(|err| panic!("{err}\n{example}"));
                    check_config(&config).unwrap_or_else(|err| panic!("{err}\n{example}"));
                    count += 1;
                }
            }
            assert!(count >= 15, "{count} examples");
            Ok(())
        });
    }

    #[test]
    fn test_check_config_publish_service() {
        let mut config = Config::default();
        config.kubernetes.publish_service = Some("proksi".to_string());

        let err = check_config(&config).unwrap_err();

        assert_eq!(
            err.to_string(),
            "kubernetes.publish_service must be namespace/name, not \"proksi\""
        );
    }

    #[test]
    fn test_check_config_tls_ciphers() {
        let mut config = Config::default();
        config.server.tls.ciphers = vec![
            "ECDHE-RSA-AES128-GCM-SHA256".to_string(),
            "ECDHE-RSA-AES128-GCM-SHA257".to_string(),
        ];

        assert_eq!(
            check_config(&config).unwrap_err().to_string(),
            "server.tls.ciphers: \"ECDHE-RSA-AES128-GCM-SHA257\" is not a TLS 1.2 cipher suite known to OpenSSL"
        );
    }

    #[test]
    fn test_check_config_trusted_proxies() {
        let mut config = Config::default();
        config.server.trusted_proxies =
            vec!["10.0.0.0/8".to_string(), "proxy.internal".to_string()];

        let err = check_config(&config).unwrap_err();

        assert_eq!(
            err.to_string(),
            "server.trusted_proxies: \"proxy.internal\" is not an address or a network (10.0.0.0/8)"
        );
    }

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
    fn test_address_used_for_http_and_https() {
        let config = config_with_host_block(
            "host \"*\" {\n  tls = false\n  listen = [\"0.0.0.0:8080\"]\n}\nhost \"a.test\" {\n  listen = [\"0.0.0.0:8080\"]\n}\n",
        );

        assert_eq!(
            check_config(&config).unwrap_err().to_string(),
            "address 0.0.0.0:8080 is used both for HTTP and HTTPS hosts"
        );
    }
}
