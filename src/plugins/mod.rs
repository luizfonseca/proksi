use anyhow::Result;
use async_trait::async_trait;
use oauth2::Oauth2;
use once_cell::sync::Lazy;
use pingora_proxy::Session;

use crate::{config::RoutePlugin, proxy_server::https_proxy::RouterContext};

pub mod jwt;
pub mod oauth2;

pub(crate) struct ProxyPlugins {
    pub oauth2: Lazy<Oauth2>,
}

pub static PLUGINS: Lazy<ProxyPlugins> = Lazy::new(|| ProxyPlugins {
    oauth2: Lazy::new(Oauth2::new),
});

#[async_trait]
pub trait MiddlewarePlugin {
    /// Create a new state for the middleware

    /// Filter requests based on the middleware's logic
    /// Return false if the request should be allowed to pass through and was not handled
    /// Return true if the request was already handled
    async fn request_filter(
        &self,
        session: &mut Session,
        state: &RouterContext,
        config: &RoutePlugin,
    ) -> Result<bool>;

    /// Filter responses (from upstream) based on the middleware's logic
    /// Return false if the request should be allowed to pass through and was not handled
    /// Return true if the request was already handled
    async fn response_filter(
        &self,
        session: &mut Session,
        state: &RouterContext,
        config: &RoutePlugin,
    ) -> Result<bool>;
}
