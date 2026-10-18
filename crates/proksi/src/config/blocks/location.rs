use std::{
    borrow::Cow,
    collections::{BTreeMap, HashMap},
};

use serde::{Deserialize, Serialize};

use crate::config::{
    host::{HealthCheck, Location, LocationTimeouts, PathRewrite},
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
    pub forward_auth: Option<PluginConfig>,
    pub request_id: Option<PluginConfig>,
    pub ip_filter: Option<PluginConfig>,
    pub cors: Option<PluginConfig>,
    pub rate_limit: Option<PluginConfig>,
    pub body_limit: Option<PluginConfig>,
    pub compress: Option<PluginConfig>,
    pub redirect: Option<PluginConfig>,
    pub respond: Option<PluginConfig>,
    pub timeouts: Option<LocationTimeouts>,
    pub rewrite: Option<PathRewrite>,
    pub health_check: Option<HealthCheck>,
    /// `rule "<name>" { ... }`, in order
    #[serde(default, deserialize_with = "crate::rules::one_or_many")]
    pub rule: Vec<crate::rules::Rule>,
}

impl LocationBlock {
    /// Converts the block of location `path`, adding the host's headers, timeouts and health check
    pub(crate) fn to_location(
        &self,
        path: &str,
        host: &super::HostBlock,
    ) -> Result<Location, String> {
        let (host_headers, host_timeouts) = (host.headers.as_ref(), host.timeouts);
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
            rewrite: match &self.rewrite {
                Some(rewrite) => {
                    rewrite.compile()?;
                    Some(rewrite.clone())
                }
                None => None,
            },
            timeouts: match self.timeouts {
                Some(timeouts) => Some(timeouts.or(host_timeouts)),
                None => host_timeouts,
            },
            health_check: match self.health_check {
                Some(health_check) => Some(health_check.or(host.health_check)),
                None => host.health_check,
            },
            rules: self.rule.clone(),
        })
    }

    /// Built-in plugins enabled in the location (validated as building blocks)
    fn plugins(&self) -> Vec<RoutePlugin> {
        [
            ("basic_auth", &self.basic_auth),
            ("oauth2", &self.oauth2),
            ("forward_auth", &self.forward_auth),
            ("request_id", &self.request_id),
            ("ip_filter", &self.ip_filter),
            ("cors", &self.cors),
            ("rate_limit", &self.rate_limit),
            ("body_limit", &self.body_limit),
            ("compress", &self.compress),
            ("redirect", &self.redirect),
            ("respond", &self.respond),
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
