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
                    Some((host.as_str(), tls.clone())),
                    format!("tcp \"{name}\" in host \"{host}\""),
                )
            })
        });

        top_level
            .chain(in_hosts)
            .map(|(name, block, host, label)| {
                block
                    .to_route(name, host.as_ref().map(|(host, tls)| (*host, tls)))
                    .map_err(|err| format!("{label}: {err}"))
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
                    upstreams: vec![upstream("db", 5432, None)],
                },
                TcpRoute {
                    name: "raw".to_string(),
                    host: Some("db.example.com".to_string()),
                    listen: "0.0.0.0:6000".to_string(),
                    tls: TcpTls::Passthrough,
                    upstreams: vec![upstream("tls-app", 443, None)],
                },
                TcpRoute {
                    name: "redis".to_string(),
                    host: Some("plain.example.com".to_string()),
                    listen: "0.0.0.0:6380".to_string(),
                    tls: TcpTls::None,
                    upstreams: vec![upstream("redis", 6379, None)],
                },
            ])
        );
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
                "tcp \"a\" {\n  listen = \"0.0.0.0:1\"\n  upstream \"b\" {}\n}\n",
                "tcp \"a\": upstream \"b\" needs a port (host:port)",
            ),
        ] {
            assert_eq!(config(hcl).tcp_routes().err().as_deref(), Some(expected), "{hcl}");
        }
    }
}
