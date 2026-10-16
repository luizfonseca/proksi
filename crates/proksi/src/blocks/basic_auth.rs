//! `basic_auth { user = "...", pass = "..." }`: answers `401` without valid credentials

use anyhow::Result;
use async_trait::async_trait;
use pingora::proxy::Session;

use crate::{
    config::RoutePlugin,
    plugins::{MiddlewarePlugin, PLUGINS},
    proxy_server::https_proxy::RouterContext,
};

use super::{priority::required, Kind, Middleware};

pub(super) struct BasicAuth {
    plugin: RoutePlugin,
}

impl BasicAuth {
    pub(super) fn new(plugin: &RoutePlugin) -> Result<Self, String> {
        required(Kind::BasicAuth, plugin, "user")?;
        required(Kind::BasicAuth, plugin, "pass")?;
        Ok(Self {
            plugin: plugin.clone(),
        })
    }
}

#[async_trait]
impl Middleware for BasicAuth {
    async fn request(&self, session: &mut Session, ctx: &mut RouterContext) -> Result<bool> {
        PLUGINS
            .basic_auth
            .request_filter(session, ctx, &self.plugin)
            .await
    }
}
