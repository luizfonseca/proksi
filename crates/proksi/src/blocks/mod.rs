//! Building blocks: everything in a location besides upstreams (see ADR 0003).
//!
//! Each block is built once per location, validated when the configuration loads, and takes
//! part in the stages of a request it needs. Blocks run by priority, from 0 (first) to 100
//! (last); the response stages run in reverse order.

use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;
use bytes::Bytes;
use pingora::{
    http::{RequestHeader, ResponseHeader},
    proxy::Session,
};

use crate::{config::host::Location, proxy_server::https_proxy::RouterContext};

use basic_auth::BasicAuth;
use body_limit::BodyLimit;
use cache::Cache;
use compress::Compress;
use headers::Headers;
use ip_filter::IpFilter;
use oauth2::Oauth2;
use priority::{plugin_priority, priority};
use rate_limit::RateLimit;
use redirect::{Redirect, Respond};
use request_id::RequestId;

mod basic_auth;
mod body_limit;
mod cache;
mod compress;
mod headers;
mod ip_filter;
mod oauth2;
mod priority;
mod rate_limit;
mod redirect;
mod request_id;

/// The stages of a request a block can take part in
// `async_trait` marks the generated methods as `#[must_use]`
#[allow(clippy::double_must_use)]
#[async_trait]
pub trait Middleware: Send + Sync {
    /// Before the request is proxied. `true` when the block answered the request, which ends it.
    async fn request(&self, _session: &mut Session, _ctx: &mut RouterContext) -> Result<bool> {
        Ok(false)
    }

    /// Each part of the request body, as it's proxied. An error ends the request.
    fn request_body(&self, _body: Option<&Bytes>, _ctx: &mut RouterContext) -> pingora::Result<()> {
        Ok(())
    }

    /// Changes the request sent to the upstream
    async fn upstream_request(
        &self,
        _session: &mut Session,
        _request: &mut RequestHeader,
        _ctx: &mut RouterContext,
    ) -> Result<()> {
        Ok(())
    }

    /// Changes the response of the upstream, before it's cached
    fn upstream_response(
        &self,
        _session: &mut Session,
        _response: &mut ResponseHeader,
        _ctx: &mut RouterContext,
    ) -> Result<()> {
        Ok(())
    }

    /// Changes the response sent to the client (including responses served from the cache)
    async fn response(
        &self,
        _session: &mut Session,
        _response: &mut ResponseHeader,
        _ctx: &mut RouterContext,
    ) -> Result<()> {
        Ok(())
    }
}

/// The kinds of blocks. Blocks with the same priority run in this order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Kind {
    RequestId,
    IpFilter,
    RateLimit,
    Redirect,
    Respond,
    BodyLimit,
    BasicAuth,
    Oauth2,
    Compress,
    Headers,
    Cache,
}

impl Kind {
    pub fn name(self) -> &'static str {
        match self {
            Kind::RequestId => "request_id",
            Kind::IpFilter => "ip_filter",
            Kind::RateLimit => "rate_limit",
            Kind::Redirect => "redirect",
            Kind::Respond => "respond",
            Kind::BodyLimit => "body_limit",
            Kind::BasicAuth => "basic_auth",
            Kind::Oauth2 => "oauth2",
            Kind::Compress => "compress",
            Kind::Headers => "headers",
            Kind::Cache => "cache",
        }
    }

    /// Where the block runs unless it sets `priority`
    pub fn default_priority(self) -> u8 {
        match self {
            Kind::RequestId | Kind::IpFilter | Kind::RateLimit => 0,
            Kind::Redirect | Kind::Respond => 5,
            Kind::BodyLimit => 10,
            Kind::BasicAuth | Kind::Oauth2 => 20,
            Kind::Compress => 40,
            Kind::Headers => 50,
            Kind::Cache => 60,
        }
    }
}

/// A block of a location
#[derive(Clone)]
pub struct Block {
    pub kind: Kind,
    pub priority: u8,
    middleware: Arc<dyn Middleware>,
}

/// The blocks of a location, in the order they run
#[derive(Clone, Default)]
pub struct Blocks(Vec<Block>);

impl Blocks {
    /// The blocks of `location`, or why its configuration is invalid
    pub fn build(location: &Location) -> Result<Self, String> {
        let mut blocks = vec![];

        for plugin in location.plugins.iter().flatten() {
            let (kind, middleware): (Kind, Arc<dyn Middleware>) = match plugin.name.as_ref() {
                "request_id" => (
                    Kind::RequestId,
                    Arc::new(RequestId {
                        plugin: plugin.clone(),
                    }),
                ),
                "ip_filter" => (Kind::IpFilter, Arc::new(IpFilter::new(plugin)?)),
                "rate_limit" => (Kind::RateLimit, Arc::new(RateLimit::new(plugin)?)),
                "body_limit" => (Kind::BodyLimit, Arc::new(BodyLimit::new(plugin)?)),
                "redirect" => (Kind::Redirect, Arc::new(Redirect::new(plugin)?)),
                "respond" => (Kind::Respond, Arc::new(Respond::new(plugin)?)),
                "basic_auth" => (Kind::BasicAuth, Arc::new(BasicAuth::new(plugin)?)),
                "oauth2" => (Kind::Oauth2, Arc::new(Oauth2::new(plugin)?)),
                "compress" => (Kind::Compress, Arc::new(Compress::new(plugin)?)),
                // Other plugins of `routes = [...]` and docker labels are ignored, as before
                _ => continue,
            };
            blocks.push(Block {
                kind,
                priority: plugin_priority(kind, plugin)?,
                middleware,
            });
        }

        if let Some(headers) = &location.headers {
            if let Some(middleware) = Headers::new(headers)? {
                blocks.push(Block {
                    kind: Kind::Headers,
                    priority: priority(Kind::Headers, headers.priority.map(u64::from))?,
                    middleware: Arc::new(middleware),
                });
            }
        }

        if let Some(cache) = &location.cache {
            if let Some(middleware) = Cache::new(cache) {
                blocks.push(Block {
                    kind: Kind::Cache,
                    priority: priority(Kind::Cache, cache.priority.map(u64::from))?,
                    middleware: Arc::new(middleware),
                });
            }
        }

        blocks.sort_by_key(|block| (block.priority, block.kind));
        Ok(Self(blocks))
    }

    /// Runs the request stage, until a block answers the request (`true`)
    pub async fn request(&self, session: &mut Session, ctx: &mut RouterContext) -> Option<Kind> {
        for block in &self.0 {
            match block.middleware.request(session, ctx).await {
                Ok(true) => return Some(block.kind),
                Ok(false) => {}
                Err(err) => tracing::debug!(block = block.kind.name(), "request: {err}"),
            }
        }
        None
    }

    /// Runs the request body stage, until a block ends the request
    pub fn request_body(
        &self,
        body: Option<&Bytes>,
        ctx: &mut RouterContext,
    ) -> Result<(), (Kind, Box<pingora::Error>)> {
        self.0.iter().try_for_each(|block| {
            block
                .middleware
                .request_body(body, ctx)
                .map_err(|err| (block.kind, err))
        })
    }

    /// The kinds of blocks that can answer a request instead of the upstream
    pub fn answering(&self) -> Vec<&'static str> {
        self.0
            .iter()
            .map(|block| block.kind)
            .filter(|kind| {
                matches!(
                    kind,
                    Kind::IpFilter
                        | Kind::RateLimit
                        | Kind::BodyLimit
                        | Kind::BasicAuth
                        | Kind::Oauth2
                )
            })
            .map(Kind::name)
            .collect()
    }

    pub async fn upstream_request(
        &self,
        session: &mut Session,
        request: &mut RequestHeader,
        ctx: &mut RouterContext,
    ) {
        for block in &self.0 {
            if let Err(err) = block
                .middleware
                .upstream_request(session, request, ctx)
                .await
            {
                tracing::debug!(block = block.kind.name(), "upstream request: {err}");
            }
        }
    }

    /// Runs in reverse order: the first block to see the request is the last to see its response
    pub fn upstream_response(
        &self,
        session: &mut Session,
        response: &mut ResponseHeader,
        ctx: &mut RouterContext,
    ) {
        for block in self.0.iter().rev() {
            if let Err(err) = block.middleware.upstream_response(session, response, ctx) {
                tracing::debug!(block = block.kind.name(), "upstream response: {err}");
            }
        }
    }

    /// Runs in reverse order: the first block to see the request is the last to see its response
    pub async fn response(
        &self,
        session: &mut Session,
        response: &mut ResponseHeader,
        ctx: &mut RouterContext,
    ) {
        for block in self.0.iter().rev() {
            if let Err(err) = block.middleware.response(session, response, ctx).await {
                tracing::debug!(block = block.kind.name(), "response: {err}");
            }
        }
    }

    /// The blocks, in the order they run on requests
    pub fn iter(&self) -> impl Iterator<Item = &Block> {
        self.0.iter()
    }
}

#[cfg(test)]
mod tests {
    use crate::config::host::Location;

    use super::{Blocks, Kind};

    fn location(json: serde_json::Value) -> Location {
        let mut location = serde_json::json!({ "path": "/" });
        location
            .as_object_mut()
            .unwrap()
            .extend(json.as_object().unwrap().clone());
        serde_json::from_value(location).unwrap()
    }

    fn basic_auth() -> serde_json::Value {
        serde_json::json!({ "name": "basic_auth", "config": { "user": "u", "pass": "p" } })
    }

    fn order(blocks: &Blocks) -> Vec<(&'static str, u8)> {
        blocks
            .iter()
            .map(|block| (block.kind.name(), block.priority))
            .collect()
    }

    #[test]
    fn test_blocks_run_by_their_default_priority() {
        let blocks = Blocks::build(&location(serde_json::json!({
            "cache": { "enabled": true },
            "headers": { "add": [{ "name": "x-a", "value": "1" }] },
            "plugins": [basic_auth(), { "name": "request_id" }],
        })))
        .unwrap();

        assert_eq!(
            order(&blocks),
            vec![
                ("request_id", 0),
                ("basic_auth", 20),
                ("headers", 50),
                ("cache", 60),
            ]
        );
    }

    #[test]
    fn test_rate_limit_runs_first_after_request_id() {
        let blocks = Blocks::build(&location(serde_json::json!({
            "plugins": [
                basic_auth(),
                { "name": "rate_limit", "config": { "requests": 10, "per": "1m" } },
                { "name": "request_id" },
            ],
        })))
        .unwrap();

        assert_eq!(
            order(&blocks),
            vec![("request_id", 0), ("rate_limit", 0), ("basic_auth", 20)]
        );
    }

    #[test]
    fn test_body_limit_runs_before_authentication() {
        let blocks = Blocks::build(&location(serde_json::json!({
            "plugins": [
                basic_auth(),
                { "name": "body_limit", "config": { "max": "1MiB" } },
                { "name": "rate_limit", "config": { "requests": 10, "per": "1m" } },
            ],
        })))
        .unwrap();

        assert_eq!(
            order(&blocks),
            vec![("rate_limit", 0), ("body_limit", 10), ("basic_auth", 20)]
        );
    }

    #[test]
    fn test_priority_overrides_the_default() {
        let blocks = Blocks::build(&location(serde_json::json!({
            "headers": { "add": [{ "name": "x-a", "value": "1" }], "priority": 10 },
            "plugins": [basic_auth(), { "name": "request_id", "config": { "priority": 60 } }],
        })))
        .unwrap();

        assert_eq!(
            order(&blocks),
            vec![("headers", 10), ("basic_auth", 20), ("request_id", 60)]
        );
    }

    #[test]
    fn test_blocks_with_the_same_priority_run_in_table_order() {
        let blocks = Blocks::build(&location(serde_json::json!({
            "headers": { "add": [{ "name": "x-a", "value": "1" }], "priority": 20 },
            "plugins": [{ "name": "request_id", "config": { "priority": 20 } }, basic_auth()],
        })))
        .unwrap();

        assert_eq!(
            blocks.iter().map(|b| b.kind).collect::<Vec<_>>(),
            vec![Kind::RequestId, Kind::BasicAuth, Kind::Headers]
        );
    }

    #[test]
    fn test_priority_must_be_between_0_and_100() {
        let err = Blocks::build(&location(serde_json::json!({
            "plugins": [{ "name": "request_id", "config": { "priority": 101 } }],
        })))
        .err();

        assert_eq!(
            err.as_deref(),
            Some("request_id: priority must be between 0 and 100")
        );
    }

    #[test]
    fn test_invalid_block_settings_are_rejected() {
        for (plugin, expected) in [
            (
                serde_json::json!({ "name": "basic_auth", "config": { "user": "u" } }),
                "basic_auth: `pass` is required",
            ),
            (
                serde_json::json!({ "name": "oauth2", "config": { "provider": "github" } }),
                "oauth2: `client_id` is required",
            ),
            (
                serde_json::json!({ "name": "oauth2", "config": {
                    "provider": "other", "client_id": "a", "client_secret": "b", "jwt_secret": "c"
                } }),
                "oauth2: unknown provider \"other\" (github, workos)",
            ),
        ] {
            let err = Blocks::build(&location(serde_json::json!({ "plugins": [plugin] }))).err();
            assert_eq!(err.as_deref(), Some(expected));
        }
    }

    #[test]
    fn test_disabled_cache_and_empty_headers_add_no_blocks() {
        let blocks = Blocks::build(&location(serde_json::json!({
            "cache": { "enabled": false },
            "headers": { "add": [], "remove": [] },
            "plugins": [{ "name": "unknown" }],
        })))
        .unwrap();

        assert!(blocks.iter().next().is_none());
    }
}
