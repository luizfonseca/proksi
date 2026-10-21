use std::{borrow::Cow, collections::HashMap, sync::LazyLock};

use anyhow::{anyhow, Result};
use async_trait::async_trait;
use pingora::proxy::Session;

use crate::{basic_auth::filter::BasicAuth, oauth2::filter::Oauth2, RequestCtx, RoutePlugin};

pub(crate) struct ProxyPlugins {
    pub basic_auth: LazyLock<BasicAuth>,
    pub oauth2: LazyLock<Oauth2>,
}

/// Static plugin registry (plugins that don't generate a new instance for each request)
pub static PLUGINS: LazyLock<ProxyPlugins> = LazyLock::new(|| ProxyPlugins {
    basic_auth: LazyLock::new(BasicAuth::new),
    oauth2: LazyLock::new(Oauth2::new),
});

/// Get a required configuration value from a plugin config
pub(crate) fn get_required_config(
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
        state: &mut dyn RequestCtx,
        config: &RoutePlugin,
    ) -> Result<bool>;

    /// Filter responses (from upstream) based on the middleware's logic
    /// Return false if the request should be allowed to pass through and was not handled
    /// Return true if the request was already handled
    async fn response_filter(
        &self,
        session: &mut Session,
        state: &mut dyn RequestCtx,
        config: &RoutePlugin,
    ) -> Result<bool>;
}
