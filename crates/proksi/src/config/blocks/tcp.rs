//! `tcp "<name>" { listen = "...", upstream "<host:port>" {} }`, at the top level or in a host

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::config::{
    host::{parse_upstream_address, HostTls},
    tcp::{TcpRoute, TcpTls},
    RouteUpstream,
};

/// `tcp "<name>" { ... }`
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TcpBlock {
    /// The address connections are accepted on (`ip:port`)
    pub listen: String,
    /// `"passthrough"`: forward TLS untouched instead of terminating it (in a host)
    pub tls: Option<String>,
    #[serde(default)]
    pub upstream: BTreeMap<String, TcpUpstreamBlock>,
}

/// `upstream "<host:port>" { ... }` of a `tcp` block
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TcpUpstreamBlock {
    pub weight: Option<i8>,
}

impl TcpBlock {
    /// Converts the block `name`, of `host` (its name and TLS settings) or at the top level
    pub fn to_route(&self, name: &str, host: Option<(&str, &HostTls)>) -> Result<TcpRoute, String> {
        let tls = match (host, self.tls.as_deref()) {
            (None, Some(_)) => {
                return Err("`tls` is only for tcp blocks inside a host".to_string());
            }
            (Some((_, host_tls)), None) if host_tls.enabled => TcpTls::Terminate,
            // At the top level, or in a host with `tls = false`
            (None | Some(_), None) => TcpTls::None,
            (Some((_, host_tls)), Some("passthrough")) if host_tls.enabled => TcpTls::Passthrough,
            (Some(_), Some("passthrough")) => {
                return Err("`tls = \"passthrough\"` needs a host with TLS".to_string());
            }
            (Some(_), Some(other)) => {
                return Err(format!("`tls` can only be \"passthrough\", not {other:?}"));
            }
        };

        let upstreams = self
            .upstream
            .iter()
            .map(|(label, upstream)| {
                let (ip, port) = parse_upstream_address(label)?;
                Ok(RouteUpstream {
                    ip: ip.into(),
                    port,
                    weight: upstream.weight,
                    ..RouteUpstream::default()
                })
            })
            .collect::<Result<Vec<_>, String>>()?;

        Ok(TcpRoute {
            name: name.to_string(),
            host: host.map(|(name, _)| name.to_string()),
            listen: self.listen.clone(),
            tls,
            upstreams,
        })
    }
}
