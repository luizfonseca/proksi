//! `host "<name>" { location "<path>" { ... } }` blocks (ADR 0003), as written in HCL or YAML.
//! Each block converts into the model in [`crate::config::host`].

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use super::host::{HealthCheck, Host, LocationTimeouts};

mod headers;
mod location;
mod tcp;
mod tls;
mod upstream;

pub use headers::HeadersBlock;
pub use location::LocationBlock;
pub use tcp::TcpBlock;
pub use tls::TlsSetting;

/// `host "<name>" { ... }`
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HostBlock {
    /// Addresses the host is served on (`ip:port`), instead of the default ones
    #[serde(default)]
    pub listen: Vec<String>,
    /// `tls = false` (plain HTTP) or `tls { ... }`
    pub tls: Option<TlsSetting>,
    /// Headers for every location of the host
    pub headers: Option<HeadersBlock>,
    /// Timeouts for every location of the host (a location can set its own)
    pub timeouts: Option<LocationTimeouts>,
    /// How the upstreams of every location and `tcp` block of the host are checked
    pub health_check: Option<HealthCheck>,
    #[serde(default)]
    pub location: BTreeMap<String, LocationBlock>,
    /// TCP proxying with the host's certificate (or its TLS forwarded, or plain TCP)
    #[serde(default)]
    pub tcp: BTreeMap<String, TcpBlock>,
    /// `rule "<name>" { ... }`, in order: before the location is chosen
    #[serde(default, deserialize_with = "crate::rules::one_or_many")]
    pub rule: Vec<crate::rules::Rule>,
}

impl HostBlock {
    /// Converts the block of host `name` into a host
    pub fn to_host(&self, name: &str) -> Result<Host, String> {
        let locations = self
            .location
            .iter()
            .map(|(path, location)| {
                location
                    .to_location(path, self)
                    .map_err(|err| format!("location \"{path}\": {err}"))
            })
            .collect::<Result<Vec<_>, _>>()?;

        Ok(Host {
            name: name.to_string(),
            listen: self.listen.clone(),
            tls: self
                .tls
                .as_ref()
                .map(TlsSetting::to_host_tls)
                .unwrap_or_default(),
            locations,
            rules: self.rule.clone(),
        })
    }
}

#[cfg(test)]
mod tests {
    use std::{borrow::Cow, collections::BTreeMap};

    use figment::providers::{Format, Yaml};
    use serde::Deserialize;

    use crate::config::{
        hcl::HclFormat,
        host::{Host, HostTls, Location},
        RouteHeader, RouteHeaderAdd, RouteHeaderRemove, RoutePlugin, RouteSslPath, RouteUpstream,
    };

    use super::HostBlock;

    #[derive(Deserialize)]
    struct Doc {
        host: BTreeMap<String, HostBlock>,
    }

    const HCL: &str = r#"
host "app.test" {
  tls {
    certificate {
      pem = "/certs/app.pem"
      key = "/certs/app.key"
    }
    self_signed_on_failure = true
  }

  headers {
    response {
      add    = { "x-host" = "app" }
      remove = ["Server"]
    }
  }

  location "/v1/session" {
    upstream "directory:8080" {}
  }

  location "/*" {
    headers {
      response {
        add = { "x-location" = "default" }
      }
    }
    request_id {}
    upstream "10.0.0.1:3000" {
      weight = 2
      sni    = "app.internal"
      headers {
        add = { "x-forwarded-by" = "proksi" }
      }
    }
  }
}
"#;

    const YAML: &str = r#"
host:
  app.test:
    tls:
      certificate: { pem: /certs/app.pem, key: /certs/app.key }
      self_signed_on_failure: true
    headers:
      response: { add: { x-host: app }, remove: [Server] }
    location:
      /v1/session:
        upstream: { "directory:8080": {} }
      /*:
        headers: { response: { add: { x-location: default } } }
        request_id: {}
        upstream:
          "10.0.0.1:3000":
            weight: 2
            sni: app.internal
            headers: { add: { x-forwarded-by: proksi } }
"#;

    fn header(name: &str, value: &str) -> RouteHeaderAdd {
        RouteHeaderAdd {
            name: Cow::Owned(name.to_string()),
            value: Cow::Owned(value.to_string()),
        }
    }

    fn host_headers(extra: &[RouteHeaderAdd]) -> Option<RouteHeader> {
        let mut add = vec![header("x-host", "app")];
        add.extend_from_slice(extra);
        Some(RouteHeader {
            add: Some(add),
            remove: Some(vec![RouteHeaderRemove {
                name: Cow::Owned("Server".to_string()),
            }]),
            priority: None,
        })
    }

    fn expected() -> Host {
        Host {
            name: "app.test".to_string(),
            listen: vec![],
            rules: vec![],
            tls: HostTls {
                enabled: true,
                certificate: Some(RouteSslPath {
                    pem: "/certs/app.pem".into(),
                    key: "/certs/app.key".into(),
                }),
                self_signed_on_failure: true,
            },
            // Sorted by path
            locations: vec![
                Location {
                    path: "/*".to_string(),
                    exact: false,
                    upstreams: vec![RouteUpstream {
                        ip: Cow::Owned("10.0.0.1".to_string()),
                        port: 3000,
                        network: None,
                        weight: Some(2),
                        sni: Some("app.internal".to_string()),
                        headers: Some(RouteHeader {
                            add: Some(vec![header("x-forwarded-by", "proksi")]),
                            remove: None,
                            priority: None,
                        }),
                        name: None,
                    }],
                    headers: host_headers(&[header("x-location", "default")]),
                    plugins: Some(vec![RoutePlugin {
                        name: Cow::Owned("request_id".to_string()),
                        config: Some(std::collections::HashMap::new()),
                    }]),
                    cache: None,
                    timeouts: None,
                    rewrite: None,
                    health_check: None,
                    rules: vec![],
                },
                Location {
                    path: "/v1/session".to_string(),
                    exact: false,
                    upstreams: vec![RouteUpstream {
                        ip: Cow::Owned("directory".to_string()),
                        port: 8080,
                        network: None,
                        weight: None,
                        sni: None,
                        headers: None,
                        name: None,
                    }],
                    headers: host_headers(&[]),
                    plugins: Some(vec![]),
                    cache: None,
                    timeouts: None,
                    rewrite: None,
                    health_check: None,
                    rules: vec![],
                },
            ],
        }
    }

    fn to_host(doc: &Doc) -> Result<Host, String> {
        doc.host.get("app.test").unwrap().to_host("app.test")
    }

    #[test]
    fn test_hcl_host_block() {
        let doc = HclFormat::from_str::<Doc>(HCL).unwrap();
        assert_eq!(to_host(&doc), Ok(expected()));
    }

    #[test]
    fn test_yaml_host_block() {
        let doc = Yaml::from_str::<Doc>(YAML).unwrap();
        assert_eq!(to_host(&doc), Ok(expected()));
    }

    #[test]
    fn test_unknown_attributes_are_rejected() {
        let err = HclFormat::from_str::<Doc>("host \"app.test\" {\n  locaton \"/\" {\n  }\n}\n")
            .err()
            .unwrap();

        assert!(err.to_string().contains("unknown field `locaton`"), "{err}");
    }

    #[test]
    fn test_invalid_upstream_label() {
        let doc = HclFormat::from_str::<Doc>(
            "host \"app.test\" {\n  location \"/\" {\n    upstream \"10.0.0.1\" {}\n  }\n}\n",
        )
        .unwrap();

        assert_eq!(
            to_host(&doc),
            Err("location \"/\": upstream \"10.0.0.1\" needs a port (host:port)".to_string())
        );
    }

    #[test]
    fn test_catch_all_host_without_tls() {
        let doc = HclFormat::from_str::<Doc>(
            "host \"*\" {\n  tls = false\n  location \"/\" {\n    upstream \"app:8080\" {}\n  }\n}\n",
        )
        .unwrap();

        let host = doc.host.get("*").unwrap().to_host("*").unwrap();

        assert_eq!(host.name, "*");
        assert!(!host.tls.enabled);
        assert_eq!(crate::routing::validate::validate_host(&host), Ok(()));
    }

    #[test]
    fn test_rate_limit_block() {
        let doc = HclFormat::from_str::<Doc>(
            "host \"a.test\" {\n  location \"/\" {\n    rate_limit {\n      requests = 10\n      per = \"1m\"\n    }\n  }\n}\n",
        )
        .unwrap();

        let host = doc.host.get("a.test").unwrap().to_host("a.test").unwrap();
        let plugins = host.locations[0].plugins.clone().unwrap();

        assert_eq!(plugins[0].name, "rate_limit");
        let config = plugins[0].config.as_ref().unwrap();
        assert_eq!(config["requests"], 10);
        assert_eq!(config["per"], "1m");
    }

    #[test]
    fn test_body_limit_block() {
        let doc = HclFormat::from_str::<Doc>(
            "host \"a.test\" {\n  location \"/\" {\n    body_limit {\n      max = \"1MiB\"\n    }\n  }\n}\n",
        )
        .unwrap();

        let host = doc.host.get("a.test").unwrap().to_host("a.test").unwrap();
        let plugins = host.locations[0].plugins.clone().unwrap();

        assert_eq!(plugins[0].name, "body_limit");
        assert_eq!(plugins[0].config.as_ref().unwrap()["max"], "1MiB");
    }

    #[test]
    fn test_host_listen_addresses() {
        let doc = HclFormat::from_str::<Doc>(
            "host \"a.test\" {\n  listen = [\"0.0.0.0:8080\", \"[::]:8080\"]\n}\n",
        )
        .unwrap();

        let host = doc.host.get("a.test").unwrap().to_host("a.test").unwrap();

        assert_eq!(host.listen, vec!["0.0.0.0:8080", "[::]:8080"]);
    }

    #[test]
    fn test_locations_inherit_the_timeouts_of_their_host() {
        let doc = HclFormat::from_str::<Doc>(
            r#"
host "app.test" {
  timeouts {
    read_secs  = 10
    write_secs = 20
  }
  location "/upload" {
    timeouts {
      read_secs = 300
    }
  }
  location "/" {}
}
"#,
        )
        .unwrap();
        let host = doc.host["app.test"].to_host("app.test").unwrap();
        let timeouts = |path: &str| {
            host.locations
                .iter()
                .find(|location| location.path == path)
                .and_then(|location| location.timeouts)
                .map(|t| (t.read_secs, t.write_secs))
        };

        assert_eq!(timeouts("/upload"), Some((Some(300), Some(20))));
        assert_eq!(timeouts("/"), Some((Some(10), Some(20))));
    }

    #[test]
    fn test_locations_inherit_the_upstream_timeouts_of_their_host() {
        let doc = HclFormat::from_str::<Doc>(
            r#"
host "app.test" {
  timeouts {
    upstream {
      connect_secs = 2
      read_secs    = 30
    }
  }
  location "/reports" {
    timeouts {
      read_secs = 300
      upstream {
        read_secs = 600
        idle_secs = 5
      }
    }
  }
  location "/" {}
}
"#,
        )
        .unwrap();
        let host = doc.host["app.test"].to_host("app.test").unwrap();
        let upstream = |path: &str| {
            host.locations
                .iter()
                .find(|location| location.path == path)
                .and_then(|location| location.timeouts)
                .and_then(|timeouts| timeouts.upstream)
                .map(|t| (t.connect_secs, t.tls_secs, t.read_secs, t.write_secs, t.idle_secs))
        };

        assert_eq!(upstream("/reports"), Some((Some(2), None, Some(600), None, Some(5))));
        assert_eq!(upstream("/"), Some((Some(2), None, Some(30), None, None)));
    }

    #[test]
    fn test_locations_inherit_the_health_check_of_their_host() {
        let doc = HclFormat::from_str::<Doc>(
            r#"
host "app.test" {
  health_check {
    interval_secs = 5
  }
  location "/game" {
    health_check {
      enabled = false
    }
  }
  location "/" {}
}
"#,
        )
        .unwrap();
        let host = doc.host["app.test"].to_host("app.test").unwrap();
        let health_check = |path: &str| {
            host.locations
                .iter()
                .find(|location| location.path == path)
                .and_then(|location| location.health_check)
                .map(|check| (check.enabled, check.interval_secs))
        };

        assert_eq!(health_check("/game"), Some((Some(false), Some(5))));
        assert_eq!(health_check("/"), Some((None, Some(5))));
    }

    #[test]
    fn test_idle_timeouts_are_only_for_the_server() {
        let err = HclFormat::from_str::<Doc>(
            "host \"app.test\" {\n  timeouts {\n    idle_secs = 5\n  }\n}\n",
        )
        .err()
        .unwrap();
        assert!(err.to_string().contains("idle_secs"), "{err}");
    }
}
