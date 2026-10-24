//! Building blocks: everything in a location besides upstreams (see ADR 0003).
//!
//! The plugins (`basic_auth`, `rate_limit`...) are in `proksi_plugins`; `headers` and `cache` are
//! proksi's own. Each block is built once per location, validated when the configuration loads,
//! and takes part in the stages of a request it needs. Blocks run by priority, from 0 (first) to
//! 100 (last); the response stages run in reverse order.

use std::{sync::Arc, time::Duration};

use bytes::Bytes;
use pingora::{
    http::{RequestHeader, ResponseHeader},
    proxy::Session,
};
pub use proksi_plugins::{jwt, settings, Kind, Middleware};
use proksi_plugins::priority;

use crate::{config::host::Location, proxy_server::https_proxy::RouterContext};

use cache::Cache;
use headers::Headers;

mod cache;
mod context;
mod headers;

/// A block of a location
#[derive(Clone)]
pub struct Block {
    pub kind: Kind,
    pub priority: u8,
    middleware: Arc<dyn Middleware>,
}

/// The blocks of a location, in the order they run
#[derive(Clone, Default)]
pub struct Blocks {
    blocks: Vec<Block>,
    /// Whether a block paces response bodies, which every other location skips
    paces: bool,
}

impl Blocks {
    /// The blocks of `location`, or why its configuration is invalid
    pub fn build(location: &Location) -> Result<Self, String> {
        let mut blocks = vec![];

        for plugin in location.plugins.iter().flatten() {
            // Other plugins of `routes = [...]` and docker labels are ignored, as before
            let Some(plugin) = proksi_plugins::build(plugin, &location.path) else {
                continue;
            };
            let plugin = plugin?;
            blocks.push(Block {
                kind: plugin.kind,
                priority: plugin.priority,
                middleware: plugin.middleware,
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
        let paces = blocks.iter().any(|block| block.kind == Kind::BandwidthLimit);
        Ok(Self { blocks, paces })
    }

    /// Runs the request stage, until a block answers the request (`true`)
    pub async fn request(&self, session: &mut Session, ctx: &mut RouterContext) -> Option<Kind> {
        for block in &self.blocks {
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
        body: &mut Option<Bytes>,
        end_of_stream: bool,
        ctx: &mut RouterContext,
    ) -> Result<(), (Kind, Box<pingora::Error>)> {
        self.blocks.iter().try_for_each(|block| {
            block
                .middleware
                .request_body(body, end_of_stream, ctx)
                .map_err(|err| (block.kind, err))
        })
    }

    /// The kinds of blocks that can answer a request instead of the upstream
    pub fn answering(&self) -> Vec<&'static str> {
        self.blocks
            .iter()
            .map(|block| block.kind)
            .filter(|kind| kind.answers())
            .map(Kind::name)
            .collect()
    }

    pub async fn upstream_request(
        &self,
        session: &mut Session,
        request: &mut RequestHeader,
        ctx: &mut RouterContext,
    ) {
        for block in &self.blocks {
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
        for block in self.blocks.iter().rev() {
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
        for block in self.blocks.iter().rev() {
            if let Err(err) = block.middleware.response(session, response, ctx).await {
                tracing::debug!(block = block.kind.name(), "response: {err}");
            }
        }
    }

    /// Whether a block paces response bodies (`bandwidth_limit`)
    pub fn paces(&self) -> bool {
        self.paces
    }

    /// How long to wait before sending `body`, part of the response: the longest any block asks
    pub fn response_body(
        &self,
        session: &Session,
        body: Option<&Bytes>,
        ctx: &mut RouterContext,
    ) -> Option<Duration> {
        if !self.paces {
            return None;
        }
        self.blocks
            .iter()
            .filter_map(|block| block.middleware.response_body(session, body, ctx))
            .max()
    }

    /// The blocks, in the order they run on requests
    pub fn iter(&self) -> impl Iterator<Item = &Block> {
        self.blocks.iter()
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
                serde_json::json!({ "name": "rate_limit", "config": { "requests": 1, "per": "1m", "window": 2 } }),
                "rate_limit: unknown field `window`, expected one of `requests`, `per`, `burst`, `priority`",
            ),
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
                "oauth2: unknown variant `other`, expected `github` or `workos`",
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
