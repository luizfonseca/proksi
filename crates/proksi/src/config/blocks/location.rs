use std::{
    borrow::Cow,
    collections::{BTreeMap, HashMap},
};

use serde::{Deserialize, Serialize};

use crate::config::{
    host::{
        HealthCheck, LoadBalancing, Location, LocationTimeouts, Mirror, PathRewrite, Retry, Sticky,
        TrafficSplit,
    },
    RouteCache, RoutePlugin,
};

use super::{
    headers::{response_headers, HeadersBlock},
    upstream::UpstreamBlock,
};

/// Settings of a built-in plugin, as in `plugins = [{ name, config }]`
type PluginConfig = HashMap<Cow<'static, str>, serde_json::Value>;

/// `location "<path>" { ... }`
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct LocationBlock {
    /// Only the path itself, not what's below it
    #[serde(default)]
    pub exact: bool,
    #[serde(default)]
    pub upstream: BTreeMap<String, UpstreamBlock>,
    pub headers: Option<HeadersBlock>,
    pub cache: Option<RouteCache>,
    #[schemars(with = "Option<proksi_plugins::settings::BasicAuth>")]
    pub basic_auth: Option<PluginConfig>,
    #[schemars(with = "Option<proksi_plugins::settings::Jwt>")]
    pub jwt: Option<PluginConfig>,
    #[schemars(with = "Option<proksi_plugins::settings::Oauth2>")]
    pub oauth2: Option<PluginConfig>,
    #[schemars(with = "Option<proksi_plugins::settings::ForwardAuth>")]
    pub forward_auth: Option<PluginConfig>,
    #[schemars(with = "Option<proksi_plugins::settings::RequestId>")]
    pub request_id: Option<PluginConfig>,
    #[schemars(with = "Option<proksi_plugins::settings::IpFilter>")]
    pub ip_filter: Option<PluginConfig>,
    #[schemars(with = "Option<proksi_plugins::settings::Cors>")]
    pub cors: Option<PluginConfig>,
    #[schemars(with = "Option<proksi_plugins::settings::RateLimit>")]
    pub rate_limit: Option<PluginConfig>,
    #[schemars(with = "Option<proksi_plugins::settings::BandwidthLimit>")]
    pub bandwidth_limit: Option<PluginConfig>,
    #[schemars(with = "Option<proksi_plugins::settings::WaitingRoom>")]
    pub waiting_room: Option<PluginConfig>,
    #[schemars(with = "Option<proksi_plugins::settings::BodyRewrite>")]
    pub body_rewrite: Option<PluginConfig>,
    #[schemars(with = "Option<proksi_plugins::settings::OpenApi>")]
    pub openapi: Option<PluginConfig>,
    #[schemars(with = "Option<proksi_plugins::settings::BodyLimit>")]
    pub body_limit: Option<PluginConfig>,
    #[schemars(with = "Option<proksi_plugins::settings::Compress>")]
    pub compress: Option<PluginConfig>,
    #[schemars(with = "Option<proksi_plugins::settings::Redirect>")]
    pub redirect: Option<PluginConfig>,
    #[schemars(with = "Option<proksi_plugins::settings::Respond>")]
    pub respond: Option<PluginConfig>,
    #[schemars(with = "Option<proksi_plugins::settings::StaticFiles>")]
    pub static_files: Option<PluginConfig>,
    pub timeouts: Option<LocationTimeouts>,
    pub rewrite: Option<PathRewrite>,
    pub health_check: Option<HealthCheck>,
    /// `rule "<name>" { ... }`, in order
    #[serde(default, deserialize_with = "crate::rules::one_or_many")]
    #[schemars(with = "crate::rules::schema::RuleList")]
    pub rule: Vec<crate::rules::Rule>,
    /// `traffic_split "<name>" { ... }`
    #[serde(default)]
    pub traffic_split: BTreeMap<String, TrafficSplitBlock>,
    /// `mirror { ... }`
    pub mirror: Option<MirrorBlock>,
    /// `round_robin` (default) or `least_connections`
    pub load_balancing: Option<LoadBalancing>,
    /// `retry { ... }`
    pub retry: Option<RetryBlock>,
}

/// `retry { ... }` in a location
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RetryBlock {
    pub attempts: Option<u8>,
    /// `"connect"`, and statuses (as `"502"` or `502`)
    pub on: Option<Vec<serde_json::Value>>,
}

impl RetryBlock {
    fn to_retry(&self) -> Retry {
        Retry {
            attempts: self.attempts.unwrap_or_else(Retry::default_attempts),
            on: self.on.as_ref().map_or_else(Retry::default_on, |on| {
                on.iter()
                    .map(|value| match value {
                        serde_json::Value::String(text) => text.clone(),
                        other => other.to_string(),
                    })
                    .collect()
            }),
        }
    }
}

/// `mirror { ... }` in a location
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MirrorBlock {
    pub percent: Option<u8>,
    /// A size such as "64KiB", or 0 for requests without a body only
    pub max_body_size: Option<serde_json::Value>,
    pub max_in_flight: Option<usize>,
    pub timeout_secs: Option<u64>,
    #[serde(default)]
    pub upstream: BTreeMap<String, UpstreamBlock>,
}

impl MirrorBlock {
    fn to_mirror(&self) -> Result<Mirror, String> {
        let in_mirror = |err: String| format!("mirror: {err}");
        let upstreams = self
            .upstream
            .iter()
            .map(|(label, upstream)| upstream.to_upstream(label))
            .collect::<Result<Vec<_>, _>>()
            .map_err(in_mirror)?;
        let max_body_size = match &self.max_body_size {
            None => Mirror::default_max_body_size(),
            Some(serde_json::Value::Number(number)) if number.as_u64() == Some(0) => 0,
            Some(serde_json::Value::String(size)) => proksi_plugins::body_limit::parse_size(size)
                .ok_or_else(|| {
                in_mirror(format!(
                    "max_body_size \"{size}\" is not a size such as \"64KiB\" or \"1MiB\""
                ))
            })?,
            Some(other) => {
                return Err(in_mirror(format!(
                    "max_body_size {other} is not a size such as \"64KiB\" or \"1MiB\", or 0"
                )));
            }
        };
        Ok(Mirror {
            percent: self.percent.unwrap_or_else(Mirror::default_percent),
            max_body_size,
            max_in_flight: self
                .max_in_flight
                .unwrap_or_else(Mirror::default_max_in_flight),
            timeout_secs: self
                .timeout_secs
                .unwrap_or_else(Mirror::default_timeout_secs),
            upstreams,
        })
    }
}

/// `traffic_split "<name>" { ... }` in a location
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TrafficSplitBlock {
    pub percent: u8,
    #[serde(default)]
    pub sticky: Sticky,
    pub cookie: Option<String>,
    #[serde(default)]
    pub upstream: BTreeMap<String, UpstreamBlock>,
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
            exact: self.exact,
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
            splits: self
                .traffic_split
                .iter()
                .map(|(name, split)| {
                    let upstreams = split
                        .upstream
                        .iter()
                        .map(|(label, upstream)| upstream.to_upstream(label))
                        .collect::<Result<Vec<_>, _>>()
                        .map_err(|err| format!("traffic_split \"{name}\": {err}"))?;
                    Ok(TrafficSplit {
                        name: name.clone(),
                        percent: split.percent,
                        sticky: split.sticky,
                        cookie: split.cookie.clone(),
                        upstreams,
                    })
                })
                .collect::<Result<_, String>>()?,
            mirror: self
                .mirror
                .as_ref()
                .map(MirrorBlock::to_mirror)
                .transpose()?,
            load_balancing: self.load_balancing.unwrap_or_default(),
            retry: self.retry.as_ref().map(RetryBlock::to_retry),
        })
    }

    /// Built-in plugins enabled in the location (validated as building blocks)
    fn plugins(&self) -> Vec<RoutePlugin> {
        [
            ("basic_auth", &self.basic_auth),
            ("jwt", &self.jwt),
            ("oauth2", &self.oauth2),
            ("forward_auth", &self.forward_auth),
            ("request_id", &self.request_id),
            ("ip_filter", &self.ip_filter),
            ("cors", &self.cors),
            ("rate_limit", &self.rate_limit),
            ("bandwidth_limit", &self.bandwidth_limit),
            ("waiting_room", &self.waiting_room),
            ("body_rewrite", &self.body_rewrite),
            ("openapi", &self.openapi),
            ("body_limit", &self.body_limit),
            ("compress", &self.compress),
            ("redirect", &self.redirect),
            ("respond", &self.respond),
            ("static_files", &self.static_files),
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
