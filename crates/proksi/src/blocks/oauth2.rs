//! `oauth2 { provider = "github", ... }`: signs users in with an `OAuth2` provider

use anyhow::Result;
use async_trait::async_trait;
use pingora::{http::ResponseHeader, proxy::Session};

use crate::{
    config::RoutePlugin,
    plugins::{MiddlewarePlugin, PLUGINS},
    proxy_server::https_proxy::RouterContext,
};

use super::Middleware;

pub(super) struct Oauth2 {
    plugin: RoutePlugin,
}

impl Oauth2 {
    pub(super) fn new(plugin: &RoutePlugin) -> Result<Self, String> {
        let settings: super::settings::Oauth2 = super::settings::read("oauth2", plugin)?;
        // Validations as the plugin reads them: `type` (also written `key`) and `value`
        let mut plugin = plugin.clone();
        if let (Some(validations), Some(config)) = (settings.validations, plugin.config.as_mut()) {
            let validations = validations
                .iter()
                .map(|validation| {
                    serde_json::json!({ "type": validation.check.name(), "value": validation.value })
                })
                .collect();
            config.insert("validations".into(), serde_json::Value::Array(validations));
        }
        Ok(Self { plugin })
    }
}

#[async_trait]
impl Middleware for Oauth2 {
    async fn request(&self, session: &mut Session, ctx: &mut RouterContext) -> Result<bool> {
        PLUGINS
            .oauth2
            .request_filter(session, ctx, &self.plugin)
            .await
    }

    async fn response(
        &self,
        session: &mut Session,
        _response: &mut ResponseHeader,
        ctx: &mut RouterContext,
    ) -> Result<()> {
        PLUGINS
            .oauth2
            .response_filter(session, ctx, &self.plugin)
            .await
            .map(|_| ())
    }
}
