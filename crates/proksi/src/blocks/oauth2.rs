//! `oauth2 { provider = "github", ... }`: signs users in with an `OAuth2` provider

use anyhow::Result;
use async_trait::async_trait;
use pingora::{http::ResponseHeader, proxy::Session};

use crate::{
    config::RoutePlugin,
    plugins::{MiddlewarePlugin, PLUGINS},
    proxy_server::https_proxy::RouterContext,
};

use super::{priority::required, Kind, Middleware};

const PROVIDERS: [&str; 2] = ["github", "workos"];

pub(super) struct Oauth2 {
    plugin: RoutePlugin,
}

impl Oauth2 {
    pub(super) fn new(plugin: &RoutePlugin) -> Result<Self, String> {
        let provider = required(Kind::Oauth2, plugin, "provider")?;
        for key in ["client_id", "client_secret", "jwt_secret"] {
            required(Kind::Oauth2, plugin, key)?;
        }
        if !PROVIDERS.contains(&provider) {
            return Err(format!(
                "oauth2: unknown provider {provider:?} ({})",
                PROVIDERS.join(", ")
            ));
        }
        Ok(Self {
            plugin: plugin.clone(),
        })
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
