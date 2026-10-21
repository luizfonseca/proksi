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
        check_challenge(config, &host).map_err(|err| anyhow!("host \"{name}\": {err}"))?;
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

/// How a host of the configuration file gets its certificate is written down: wildcard hosts
/// say `dns-01` (the only challenge Let's Encrypt accepts for them), which needs the DNS
/// provider (ADR 0009)
fn check_challenge(config: &Config, host: &crate::config::host::Host) -> Result<(), String> {
    use crate::config::host::Challenge;

    if !host.tls.enabled || host.tls.certificate.is_some() {
        return Ok(());
    }
    if host.name.starts_with("*.") && host.tls.challenge != Some(Challenge::Dns01) {
        return Err(
            "wildcard certificates need tls { challenge = \"dns-01\" } (or certificate files)"
                .to_string(),
        );
    }
    if host.tls.challenge == Some(Challenge::Dns01) && config.lets_encrypt.dns.is_none() {
        return Err("challenge = \"dns-01\" needs lets_encrypt { dns { ... } }".to_string());
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
        let site = tempfile::tempdir().unwrap();
        for file in ["example.hcl", "example.yaml"] {
            let path = format!("{}/../../examples/{file}", env!("CARGO_MANIFEST_DIR"));
            // `static_files` serves a directory that exists
            let content = std::fs::read_to_string(&path)
                .unwrap()
                .replace("/var/www/app/dist", &site.path().to_string_lossy());
            let copy = site.path().join(file);
            std::fs::write(&copy, content).unwrap();
            let config = crate::config::load_for_test(&copy.to_string_lossy())
                .unwrap_or_else(|err| panic!("{file} can't be loaded: {err}"));

            check_config(&config).unwrap_or_else(|err| panic!("{file} is invalid: {err}"));
            let hosts = config.hosts();
            for host in &hosts {
                assert_eq!(
                    crate::routing::validate::validate_host(host),
                    Ok(()),
                    "{file}: {}",
                    host.name
                );
            }
            assert_eq!(hosts.len(), 7, "{file}");
            assert_eq!(
                config.tcp_routes().map(|routes| routes.len()),
                Ok(2),
                "{file}"
            );

            // The latest blocks
            let api = hosts
                .iter()
                .find(|host| host.name == "app.example.com")
                .and_then(|host| {
                    host.locations
                        .iter()
                        .find(|location| location.path == "/api")
                })
                .unwrap_or_else(|| panic!("{file}: app.example.com /api"));
            assert_eq!(api.splits.len(), 1, "{file}");
            assert!(api.mirror.is_some() && api.retry.is_some(), "{file}");
            assert_eq!(api.rules.len(), 2, "{file}");
            assert_eq!(
                api.load_balancing,
                crate::config::host::LoadBalancing::LeastConnections,
                "{file}"
            );
            assert!(config.lets_encrypt.dns.is_some(), "{file}");
        }
    }

    /// The pages of `docs/examples`: each HCL example is a whole configuration that loads, and
    /// each of its hosts is valid
    #[test]
    fn test_the_examples_pages_are_valid() {
        let pages = [
            include_str!("../../../../docs/examples/blocking-url-scanners.mdx"),
            include_str!("../../../../docs/examples/canary-release.mdx"),
            include_str!("../../../../docs/examples/single-page-app.mdx"),
            include_str!("../../../../docs/examples/behind-a-load-balancer.mdx"),
            include_str!("../../../../docs/examples/admin-area.mdx"),
            include_str!("../../../../docs/examples/public-api.mdx"),
            include_str!("../../../../docs/examples/security-headers.mdx"),
            include_str!("../../../../docs/examples/self-hosted-apps.mdx"),
        ];
        figment::Jail::expect_with(|jail| {
            jail.set_env("ADMIN_PASSWORD", "secret");
            jail.set_env("ACME_EMAIL", "ops@proksi.test");
            jail.create_dir("site")?;
            let site = jail.directory().join("site");
            let mut count = 0;
            for example in pages.iter().flat_map(|page| page.split("```hcl\n").skip(1)) {
                let example = example.split("```").next().unwrap_or_default();
                count += 1;
                example_is_valid(jail, example, &site.to_string_lossy())?;
            }
            assert!(count >= 14, "{count} examples");
            Ok(())
        });
    }

    /// Loads an example as `proksi.hcl` (its `static_files` serving `site`) and validates it
    fn example_is_valid(
        jail: &mut figment::Jail,
        example: &str,
        site: &str,
    ) -> figment::error::Result<()> {
        let example = example
            .replace("/var/www/app/dist/assets", site)
            .replace("/var/www/app/dist", site);
        jail.create_file("proksi.hcl", &example)?;
        let config = crate::config::load_for_test("proksi.hcl")
            .unwrap_or_else(|err| panic!("{err}\n{example}"));
        check_config(&config).unwrap_or_else(|err| panic!("{err}\n{example}"));
        for host in config.hosts() {
            crate::routing::validate::validate_host(&host)
                .unwrap_or_else(|err| panic!("{err}\n{example}"));
        }
        Ok(())
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
                    count += usize::from(readme_example(jail, example, file)?);
                }
            }
            assert!(count >= 15, "{count} examples");
            Ok(())
        });
    }

    /// Loads an example of the README as `file`: `false` for one that isn't loaded itself (a
    /// Docker Compose file, a file another example imports)
    fn readme_example(
        jail: &mut figment::Jail,
        example: &str,
        file: &str,
    ) -> figment::error::Result<bool> {
        // A Docker Compose file, not proksi's
        if example.starts_with("services:") {
            return Ok(false);
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
            return Ok(false);
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
        let config =
            crate::config::load_for_test(file).unwrap_or_else(|err| panic!("{err}\n{example}"));
        check_config(&config).unwrap_or_else(|err| panic!("{err}\n{example}"));
        Ok(true)
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
    fn test_challenges() {
        let host = |name: &str, tls: &str| {
            config_with_host_block(&format!(
                "host \"{name}\" {{\n  {tls}\n  location \"/\" {{\n    upstream \"10.0.0.1:3000\" {{}}\n  }}\n}}\n"
            ))
        };
        let error = |config: &Config| check_config(config).err().map(|err| err.to_string());
        let with_dns = |mut config: Config| {
            config.lets_encrypt.dns = Some(crate::config::DnsChallenge {
                provider: crate::config::DnsProviderKind::Cloudflare,
                api_token: "token".to_string(),
            });
            config
        };

        // Wildcard hosts say how their certificate is ordered
        assert_eq!(
            error(&host("*.a.test", "tls {}")),
            Some("host \"*.a.test\": wildcard certificates need tls { challenge = \"dns-01\" } (or certificate files)".to_string())
        );
        assert_eq!(
            error(&host("*.a.test", "tls {\n challenge = \"dns-01\"\n}")),
            Some(
                "host \"*.a.test\": challenge = \"dns-01\" needs lets_encrypt { dns { ... } }"
                    .to_string()
            )
        );
        assert_eq!(
            error(&with_dns(host(
                "*.a.test",
                "tls {\n challenge = \"dns-01\"\n}"
            ))),
            None
        );
        assert_eq!(
            error(&with_dns(host(
                "b.a.test",
                "tls {\n challenge = \"dns-01\"\n}"
            ))),
            None
        );
        // Or bring their certificate
        assert_eq!(
            error(&host(
                "*.a.test",
                "tls {\n certificate {\n pem = \"/a.pem\"\n key = \"/a.key\"\n }\n}"
            )),
            None
        );
        // Without TLS, nothing is ordered
        assert_eq!(error(&host("*.a.test", "tls = false")), None);
    }

    #[test]
    fn test_dns_provider() {
        #[derive(serde::Deserialize)]
        struct Doc {
            lets_encrypt: crate::config::LetsEncrypt,
        }
        let parse = |hcl: &str| {
            <crate::config::hcl::HclFormat as figment::providers::Format>::from_str::<Doc>(hcl)
                .map(|doc| doc.lets_encrypt.dns)
                .map_err(|err| err.to_string())
        };

        let dns = parse(
            "lets_encrypt {\n  email = \"a@b.test\"\n  dns {\n    provider = \"cloudflare\"\n    api_token = \"secret\"\n  }\n}\n",
        )
        .unwrap()
        .unwrap();
        assert_eq!(dns.provider, crate::config::DnsProviderKind::Cloudflare);
        assert_eq!(dns.api_token, "secret");

        let err = parse("lets_encrypt {\n  email = \"a@b.test\"\n  dns {\n    provider = \"route53\"\n    api_token = \"x\"\n  }\n}\n").unwrap_err();
        assert!(err.contains("unknown variant `route53`"), "{err}");
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
