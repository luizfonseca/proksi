//! TCP routes (`tcp` blocks, see ADR 0003): connections accepted on an address, forwarded to
//! upstreams

use serde::Serialize;

use super::{blocks::TlsSetting, Config, RouteUpstream};

/// What happens with TLS on a TCP route
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum TcpTls {
    /// Plain TCP (top-level blocks, and hosts with `tls = false`)
    None,
    /// TLS is terminated with the host's certificate; the upstream receives plain TCP
    Terminate,
    /// TLS is forwarded untouched
    Passthrough,
}

/// A `tcp` block
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct TcpRoute {
    pub name: String,
    /// The host the block is in, if any
    pub host: Option<String>,
    pub listen: String,
    pub tls: TcpTls,
    /// Protocols (ALPN) the block is chosen by, next to the host's locations
    pub alpn: Vec<String>,
    /// How long connecting to an upstream can take
    #[serde(skip)]
    pub connect_timeout: std::time::Duration,
    /// How long a connection can go without bytes either way (`None`: forever)
    #[serde(skip)]
    pub idle_timeout: Option<std::time::Duration>,
    /// How the upstreams are checked (with the host's settings)
    #[serde(skip)]
    pub health_check: Option<super::host::HealthCheck>,
    pub upstreams: Vec<RouteUpstream>,
}

impl TcpRoute {
    /// How the block is named in errors (`tcp "db" in host "a.com"`)
    pub fn label(&self) -> String {
        match &self.host {
            Some(host) => format!("tcp \"{}\" in host \"{host}\"", self.name),
            None => format!("tcp \"{}\"", self.name),
        }
    }
}

impl Config {
    /// The `tcp` blocks at the top level and in hosts
    pub fn tcp_routes(&self) -> Result<Vec<TcpRoute>, String> {
        let top_level = self
            .tcp
            .iter()
            .map(|(name, block)| (name, block, None, format!("tcp \"{name}\"")));
        let in_hosts = self.host.iter().flat_map(|(host, host_block)| {
            let tls = host_block
                .tls
                .as_ref()
                .map(TlsSetting::to_host_tls)
                .unwrap_or_default();
            host_block.tcp.iter().map(move |(name, block)| {
                (
                    name,
                    block,
                    Some((host.as_str(), tls.clone(), host_block.health_check)),
                    format!("tcp \"{name}\" in host \"{host}\""),
                )
            })
        });

        top_level
            .chain(in_hosts)
            .map(|(name, block, host, label)| {
                let mut route = block
                    .to_route(name, host.as_ref().map(|(host, tls, _)| (*host, tls)))
                    .map_err(|err| format!("{label}: {err}"))?;
                if let Some((_, _, host_health_check)) = host {
                    route.health_check = match route.health_check {
                        Some(health_check) => Some(health_check.or(host_health_check)),
                        None => host_health_check,
                    };
                }
                Ok(route)
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use figment::providers::Format;

    use crate::config::{hcl::HclFormat, Config, RouteUpstream};

    use super::{TcpRoute, TcpTls};

    fn config(hcl: &str) -> Config {
        #[derive(serde::Deserialize)]
        struct Doc {
            #[serde(default)]
            host: std::collections::BTreeMap<String, crate::config::blocks::HostBlock>,
            #[serde(default)]
            tcp: std::collections::BTreeMap<String, crate::config::blocks::TcpBlock>,
        }
        let doc = HclFormat::from_str::<Doc>(hcl).unwrap();
        let mut config = Config::default();
        config.host = doc.host;
        config.tcp = doc.tcp;
        config
    }

    fn upstream(ip: &str, port: u16, weight: Option<i8>) -> RouteUpstream {
        RouteUpstream {
            ip: ip.to_string().into(),
            port,
            weight,
            ..RouteUpstream::default()
        }
    }

    #[test]
    fn test_tcp_blocks_inherit_the_health_check_of_their_host() {
        let config = config(
            r#"
tcp "game" {
  listen = "0.0.0.0:7777"
  health_check {
    enabled = false
  }
  upstream "game:7777" {}
}

host "db.example.com" {
  health_check {
    interval_secs = 5
  }
  tcp "postgres" {
    listen = "0.0.0.0:5433"
    health_check {
      enabled = true
    }
    upstream "db:5432" {}
  }
}
"#,
        );
        let routes = config.tcp_routes().unwrap();
        let health_check = |name: &str| {
            routes
                .iter()
                .find(|route| route.name == name)
                .and_then(|route| route.health_check)
                .map(|check| (check.enabled, check.interval_secs))
        };

        assert_eq!(health_check("game"), Some((Some(false), None)));
        assert_eq!(health_check("postgres"), Some((Some(true), Some(5))));
    }

    #[test]
    fn test_tcp_routes() {
        let config = config(
            r#"
tcp "game" {
  listen = "0.0.0.0:7777"
  upstream "game-1:7777" { weight = 2 }
  upstream "game-2:7777" {}
}

host "db.example.com" {
  tcp "postgres" {
    listen = "0.0.0.0:5433"
    upstream "db:5432" {}
  }

  tcp "raw" {
    listen = "0.0.0.0:6000"
    tls    = "passthrough"
    upstream "tls-app:443" {}
  }
}

host "plain.example.com" {
  tls = false

  tcp "redis" {
    listen = "0.0.0.0:6380"
    upstream "redis:6379" {}
  }
}
"#,
        );

        assert_eq!(
            config.tcp_routes(),
            Ok(vec![
                TcpRoute {
                    name: "game".to_string(),
                    host: None,
                    listen: "0.0.0.0:7777".to_string(),
                    tls: TcpTls::None,
                    alpn: vec![],
                    connect_timeout: std::time::Duration::from_secs(5),
                    idle_timeout: None,
                    health_check: None,
                    upstreams: vec![
                        upstream("game-1", 7777, Some(2)),
                        upstream("game-2", 7777, None)
                    ],
                },
                TcpRoute {
                    name: "postgres".to_string(),
                    host: Some("db.example.com".to_string()),
                    listen: "0.0.0.0:5433".to_string(),
                    tls: TcpTls::Terminate,
                    alpn: vec![],
                    connect_timeout: std::time::Duration::from_secs(5),
                    idle_timeout: None,
                    health_check: None,
                    upstreams: vec![upstream("db", 5432, None)],
                },
                TcpRoute {
                    name: "raw".to_string(),
                    host: Some("db.example.com".to_string()),
                    listen: "0.0.0.0:6000".to_string(),
                    tls: TcpTls::Passthrough,
                    alpn: vec![],
                    connect_timeout: std::time::Duration::from_secs(5),
                    idle_timeout: None,
                    health_check: None,
                    upstreams: vec![upstream("tls-app", 443, None)],
                },
                TcpRoute {
                    name: "redis".to_string(),
                    host: Some("plain.example.com".to_string()),
                    listen: "0.0.0.0:6380".to_string(),
                    tls: TcpTls::None,
                    alpn: vec![],
                    connect_timeout: std::time::Duration::from_secs(5),
                    idle_timeout: None,
                    health_check: None,
                    upstreams: vec![upstream("redis", 6379, None)],
                },
            ])
        );
    }

    #[test]
    fn test_tcp_alpn() {
        let config = config(
            "host \"a.test\" {\n  tcp \"game\" {\n    listen = \"0.0.0.0:443\"\n    alpn = [\"game/1\", \"game/2\"]\n    upstream \"game:7777\" {}\n  }\n}\n",
        );

        let routes = config.tcp_routes().unwrap();
        assert_eq!(routes[0].alpn, vec!["game/1", "game/2"]);
    }

    #[test]
    fn test_invalid_tcp_blocks() {
        for (hcl, expected) in [
            (
                "tcp \"a\" {\n  listen = \"0.0.0.0:1\"\n  tls = \"passthrough\"\n  upstream \"b:1\" {}\n}\n",
                "tcp \"a\": `tls` is only for tcp blocks inside a host",
            ),
            (
                "host \"h\" {\n  tls = false\n  tcp \"a\" {\n    listen = \"0.0.0.0:1\"\n    tls = \"passthrough\"\n    upstream \"b:1\" {}\n  }\n}\n",
                "tcp \"a\" in host \"h\": `tls = \"passthrough\"` needs a host with TLS",
            ),
            (
                "host \"h\" {\n  tcp \"a\" {\n    listen = \"0.0.0.0:1\"\n    tls = \"terminate\"\n    upstream \"b:1\" {}\n  }\n}\n",
                "tcp \"a\" in host \"h\": `tls` can only be \"passthrough\", not \"terminate\"",
            ),
            (
                "tcp \"a\" {\n  listen = \"0.0.0.0:1\"\n  alpn = [\"x\"]\n  upstream \"b:1\" {}\n}\n",
                "tcp \"a\": `alpn` is only for tcp blocks in a host with TLS (not passthrough)",
            ),
            (
                "host \"h\" {\n  tcp \"a\" {\n    listen = \"0.0.0.0:1\"\n    tls = \"passthrough\"\n    alpn = [\"x\"]\n    upstream \"b:1\" {}\n  }\n}\n",
                "tcp \"a\" in host \"h\": `alpn` is only for tcp blocks in a host with TLS (not passthrough)",
            ),
            (
                "host \"h\" {\n  tcp \"a\" {\n    listen = \"0.0.0.0:1\"\n    alpn = [\"h2\"]\n    upstream \"b:1\" {}\n  }\n}\n",
                "tcp \"a\" in host \"h\": `alpn` can't be \"h2\" or \"http/1.1\" (used by HTTPS)",
            ),
            (
                "host \"h\" {\n  tcp \"a\" {\n    listen = \"0.0.0.0:1\"\n    alpn = [\"\"]\n    upstream \"b:1\" {}\n  }\n}\n",
                "tcp \"a\" in host \"h\": `alpn` protocols must be 1 to 255 bytes long",
            ),
            (
                "tcp \"a\" {\n  listen = \"0.0.0.0:1\"\n  upstream \"b\" {}\n}\n",
                "tcp \"a\": upstream \"b\" needs a port (host:port)",
            ),
        ] {
            assert_eq!(config(hcl).tcp_routes().err().as_deref(), Some(expected), "{hcl}");
        }
    }
}
