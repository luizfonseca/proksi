use std::{borrow::Cow, collections::BTreeMap};

use serde::{Deserialize, Serialize};

use crate::config::{RouteHeader, RouteUpstream, host::parse_upstream_address};

use super::headers::header;

/// `upstream "<host:port>" { ... }`
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct UpstreamBlock {
    pub weight: Option<i8>,
    /// Docker network to reach the upstream through
    pub network: Option<String>,
    /// SNI sent to the upstream (for HTTPS upstreams)
    pub sni: Option<String>,
    pub headers: Option<UpstreamHeadersBlock>,
}

/// Headers added to the request sent to the upstream
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct UpstreamHeadersBlock {
    #[serde(default)]
    pub add: BTreeMap<String, String>,
}

impl UpstreamBlock {
    /// Converts the block of upstream `label` (its address)
    pub(super) fn to_upstream(&self, label: &str) -> Result<RouteUpstream, String> {
        let (ip, port) = parse_upstream_address(label)?;
        Ok(RouteUpstream {
            ip: Cow::Owned(ip),
            port,
            network: self.network.clone(),
            weight: self.weight,
            sni: self.sni.clone(),
            headers: self.headers.as_ref().map(|headers| RouteHeader {
                add: Some(
                    headers
                        .add
                        .iter()
                        .map(|(name, value)| header(name, value))
                        .collect(),
                ),
                remove: None,
                priority: None,
            }),
            name: None,
        })
    }
}
