use std::{borrow::Cow, collections::HashMap};

use anyhow::{anyhow, Result};
use async_trait::async_trait;
use basic_auth::BasicAuth;
use oauth2::Oauth2;
use once_cell::sync::Lazy;
use pingora::proxy::Session;

use crate::{config::RoutePlugin, proxy_server::https_proxy::RouterContext};

pub mod basic_auth;
pub mod jwt;
pub mod oauth2;

pub(crate) struct ProxyPlugins {
    pub basic_auth: Lazy<BasicAuth>,
    pub oauth2: Lazy<Oauth2>,
}

/// Static plugin registry (plugins that don't generate a new instance for each request)
pub static PLUGINS: Lazy<ProxyPlugins> = Lazy::new(|| ProxyPlugins {
    basic_auth: Lazy::new(BasicAuth::new),
    oauth2: Lazy::new(Oauth2::new),
});

/// Get a required configuration value from a plugin config
fn get_required_config(
    plugin_config: &HashMap<Cow<'static, str>, serde_json::Value>,
    key: &str,
) -> Result<String> {
    plugin_config
        .get(key)
        .and_then(|v| v.as_str())
        .map(ToString::to_string)
        .ok_or_else(|| anyhow!("Missing or invalid {key}"))
}

// `async_trait` marks the generated methods as `#[must_use]`
#[allow(clippy::double_must_use)]
#[async_trait]
pub trait MiddlewarePlugin {
    /// Create a new state for the middleware
    /// Filter requests based on the middleware's logic
    /// Return false if the request should be allowed to pass through and was not handled
    /// Return true if the request was already handled
    async fn request_filter(
        &self,
        session: &mut Session,
        state: &mut RouterContext,
        config: &RoutePlugin,
    ) -> Result<bool>;

    /// Filter responses (from upstream) based on the middleware's logic
    /// Return false if the request should be allowed to pass through and was not handled
    /// Return true if the request was already handled
    async fn response_filter(
        &self,
        session: &mut Session,
        state: &mut RouterContext,
        config: &RoutePlugin,
    ) -> Result<bool>;
}
