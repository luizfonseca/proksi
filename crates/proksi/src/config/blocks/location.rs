use std::{
    borrow::Cow,
    collections::{BTreeMap, HashMap},
};

use serde::{Deserialize, Serialize};

use crate::config::{
    host::{Location, LocationTimeouts},
    RouteCache, RoutePlugin,
};

use super::{
    headers::{response_headers, HeadersBlock},
    upstream::UpstreamBlock,
};

/// Settings of a built-in plugin, as in `plugins = [{ name, config }]`
type PluginConfig = HashMap<Cow<'static, str>, serde_json::Value>;

/// `location "<path>" { ... }`
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LocationBlock {
    #[serde(default)]
    pub upstream: BTreeMap<String, UpstreamBlock>,
    pub headers: Option<HeadersBlock>,
    pub cache: Option<RouteCache>,
    pub basic_auth: Option<PluginConfig>,
    pub oauth2: Option<PluginConfig>,
    pub request_id: Option<PluginConfig>,
    pub rate_limit: Option<PluginConfig>,
    pub body_limit: Option<PluginConfig>,
    pub timeouts: Option<LocationTimeouts>,
}

impl LocationBlock {
    /// Converts the block of location `path`, adding the host's headers and timeouts
    pub(crate) fn to_location(
        &self,
        path: &str,
        host_headers: Option<&HeadersBlock>,
        host_timeouts: Option<LocationTimeouts>,
    ) -> Result<Location, String> {
        let upstreams = self
            .upstream
            .iter()
            .map(|(label, upstream)| upstream.to_upstream(label))
            .collect::<Result<Vec<_>, _>>()?;

        Ok(Location {
            path: path.to_string(),
            exact: false,
            upstreams,
            headers: Some(response_headers([host_headers, self.headers.as_ref()])),
            plugins: Some(self.plugins()),
            cache: self.cache.clone(),
            timeouts: match self.timeouts {
                Some(timeouts) => Some(timeouts.or(host_timeouts)),
                None => host_timeouts,
            },
        })
    }

    /// Built-in plugins enabled in the location (validated as building blocks)
    fn plugins(&self) -> Vec<RoutePlugin> {
        [
            ("basic_auth", &self.basic_auth),
            ("oauth2", &self.oauth2),
            ("request_id", &self.request_id),
            ("rate_limit", &self.rate_limit),
            ("body_limit", &self.body_limit),
        ]
        .into_iter()
        .filter_map(|(name, config)| {
            config.as_ref().map(|config| RoutePlugin {
                name: Cow::Borrowed(name),
                config: Some(config.clone()),
            })
        })
        .collect()
    }
}
