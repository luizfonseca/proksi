//! `request_id {}`: adds an `X-Request-Id` header to requests and responses

use anyhow::Result;
use async_trait::async_trait;
use pingora::{
    http::{RequestHeader, ResponseHeader},
    proxy::Session,
};

use crate::{
    config::RoutePlugin,
    plugins::{MiddlewarePlugin, PLUGINS},
    proxy_server::https_proxy::RouterContext,
};

use super::Middleware;

pub(super) struct RequestId {
    pub(super) plugin: RoutePlugin,
}

#[async_trait]
impl Middleware for RequestId {
    async fn request(&self, session: &mut Session, ctx: &mut RouterContext) -> Result<bool> {
        PLUGINS
            .request_id
            .request_filter(session, ctx, &self.plugin)
            .await
    }

    async fn upstream_request(
        &self,
        session: &mut Session,
        request: &mut RequestHeader,
        ctx: &mut RouterContext,
    ) -> Result<()> {
        PLUGINS
            .request_id
            .upstream_request_filter(session, request, ctx)
            .await
    }

    fn upstream_response(
        &self,
        session: &mut Session,
        response: &mut ResponseHeader,
        ctx: &mut RouterContext,
    ) -> Result<()> {
        PLUGINS
            .request_id
            .upstream_response_filter(session, response, ctx)
    }
}
