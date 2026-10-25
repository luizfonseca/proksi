//! Proksi's built-in plugins: the building blocks of a location besides `headers` and `cache`
//! (see ADR 0003), such as `basic_auth`, `rate_limit` or `static_files`.
//!
//! Each plugin is built once per location from its [`RoutePlugin`], validated when the
//! configuration loads, and takes part in the stages of a request it needs ([`Middleware`]). What
//! a plugin needs from proksi for a request (its host, its client, applying the location's
//! response blocks to an answer) it asks through [`RequestCtx`].

use std::{any::Any, borrow::Cow, collections::HashMap, net::IpAddr, sync::Arc, time::Duration};

use anyhow::Result;
use async_trait::async_trait;
use bytes::Bytes;
use pingora::{
    http::{RequestHeader, ResponseHeader},
    proxy::Session,
};
use serde::{Deserialize, Serialize};

use bandwidth_limit::BandwidthLimit;
use body_rewrite::BodyRewrite;
use openapi::OpenApi;
use basic_auth::BasicAuth;
use body_limit::BodyLimit;
use compress::Compress;
use cors::Cors;
use forward_auth::ForwardAuth;
use ip_filter::IpFilter;
use jwt::Jwt;
use oauth2::Oauth2;
use rate_limit::RateLimit;
use redirect::{Redirect, Respond};
use request_id::RequestId;
use static_files::StaticFiles;
use waiting_room::WaitingRoom;

#[cfg(any(test, feature = "test-util"))]
pub mod allocations;
mod bandwidth_limit;
mod basic_auth;
pub mod body_limit;
mod body_rewrite;
pub mod openapi;
mod compress;
mod cors;
mod filter;
mod forward_auth;
mod ip_filter;
pub mod ip_set;
pub mod jwt;
mod oauth2;
mod priority;
mod rate_limit;
mod redirect;
mod request_id;
pub mod respond;
pub mod settings;
mod static_files;
mod waiting_room;

pub use priority::{plugin_priority, priority};

// The tests count allocations (see `allocations`)
#[cfg(test)]
#[global_allocator]
static GLOBAL: allocations::Counting = allocations::Counting;

/// A plugin of a location, as configured: its name, and its settings
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct RoutePlugin {
    /// The name of the plugin (must be a valid plugin name)
    pub name: Cow<'static, str>,

    /// The configuration for the plugin - we are not enforcing a specific format.
    /// Each plugin is in charge of parsing the configuration.
    /// The configuration is a key-value pair where the key is a string and
    /// the value is a JSON object (ex: `{ "key": "value" }`)
    pub config: Option<HashMap<Cow<'static, str>, serde_json::Value>>,
}

/// What a plugin can ask of proksi about the request it's handling
#[async_trait]
pub trait RequestCtx: Send {
    /// The host the request is for, without its port
    fn host(&self) -> &str;

    /// The request's ID, once `request_id` gave it one
    fn request_id(&self) -> Option<&http::HeaderValue>;

    fn set_request_id(&mut self, id: http::HeaderValue);

    /// Counts `bytes` more of the request's body, and returns how many were read so far
    fn count_request_body(&mut self, bytes: u64) -> u64;

    /// The client, behind trusted proxies the one they forwarded
    fn client(&self, session: &Session) -> Option<IpAddr>;

    /// Whether `peer` is a trusted proxy, whose forwarded headers are kept
    fn is_trusted(&self, peer: IpAddr) -> bool;

    /// `path` as locations read it (`//a/./b` is `/a/b`)
    fn normalize_path<'a>(&self, path: &'a str) -> Cow<'a, str>;

    /// Changes `response`, an answer of proksi itself, as the location's response blocks and the
    /// rules change upstreams' responses (its `headers`, its CORS headers...)
    async fn response_blocks(&mut self, session: &mut Session, response: &mut ResponseHeader);

    /// Adds a header to the response, whoever answers the request
    fn add_response_header(&mut self, name: http::HeaderName, value: http::HeaderValue);

    /// What blocks keep for this request, by block: see [`block_state`]
    fn block_states(&mut self) -> &mut Vec<(usize, Box<dyn Any + Send + Sync>)>;

    /// The store shared by the replicas (memory, Redis or SQL)
    fn store(&self) -> Arc<dyn proksi_store::Store>;

    /// How long to wait before sending `body`, part of an answer of proksi itself, as the
    /// location's blocks and the rules pace upstreams' responses (`bandwidth_limit`)
    fn response_body_delay(&mut self, session: &Session, body: Option<&Bytes>) -> Option<Duration>;
}

/// The stages of a request a block can take part in
// `async_trait` marks the generated methods as `#[must_use]`
#[allow(clippy::double_must_use)]
#[async_trait]
pub trait Middleware: Send + Sync {
    /// Before the request is proxied. `true` when the block answered the request, which ends it.
    async fn request(&self, _session: &mut Session, _ctx: &mut dyn RequestCtx) -> Result<bool> {
        Ok(false)
    }

    /// Each part of the request body, as it's proxied, which a block can change (the last one
    /// with `end_of_stream`).
    ///
    /// # Errors
    ///
    /// An error ends the request.
    fn request_body(
        &self,
        _body: &mut Option<Bytes>,
        _end_of_stream: bool,
        _ctx: &mut dyn RequestCtx,
    ) -> pingora::Result<()> {
        Ok(())
    }

    /// Changes the request sent to the upstream
    async fn upstream_request(
        &self,
        _session: &mut Session,
        _request: &mut RequestHeader,
        _ctx: &mut dyn RequestCtx,
    ) -> Result<()> {
        Ok(())
    }

    /// Changes the response of the upstream, before it's cached
    ///
    /// # Errors
    ///
    /// An error is logged, and the response goes on unchanged by this block.
    fn upstream_response(
        &self,
        _session: &mut Session,
        _response: &mut ResponseHeader,
        _ctx: &mut dyn RequestCtx,
    ) -> Result<()> {
        Ok(())
    }

    /// Each part of the response body sent to the client (including responses served from the
    /// cache): how long to wait before sending it
    fn response_body(
        &self,
        _session: &Session,
        _body: Option<&Bytes>,
        _ctx: &mut dyn RequestCtx,
    ) -> Option<Duration> {
        None
    }

    /// Changes the response sent to the client (including responses served from the cache)
    async fn response(
        &self,
        _session: &mut Session,
        _response: &mut ResponseHeader,
        _ctx: &mut dyn RequestCtx,
    ) -> Result<()> {
        Ok(())
    }
}

/// The kinds of blocks, the plugins of this crate and proksi's own `headers` and `cache`. Blocks
/// with the same priority run in this order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Kind {
    RequestId,
    IpFilter,
    Cors,
    RateLimit,
    BandwidthLimit,
    WaitingRoom,
    Redirect,
    Respond,
    BodyLimit,
    BasicAuth,
    Jwt,
    Oauth2,
    ForwardAuth,
    BodyRewrite,
    OpenApi,
    Compress,
    Headers,
    Cache,
    StaticFiles,
}

impl Kind {
    /// Every kind, in the order blocks with the same priority run
    pub const ALL: [Kind; 19] = [
        Kind::RequestId,
        Kind::IpFilter,
        Kind::Cors,
        Kind::RateLimit,
        Kind::BandwidthLimit,
        Kind::WaitingRoom,
        Kind::Redirect,
        Kind::Respond,
        Kind::BodyLimit,
        Kind::BasicAuth,
        Kind::Jwt,
        Kind::Oauth2,
        Kind::ForwardAuth,
        Kind::BodyRewrite,
        Kind::OpenApi,
        Kind::Compress,
        Kind::Headers,
        Kind::Cache,
        Kind::StaticFiles,
    ];

    /// The kind named `name`, as written in the configuration
    #[must_use]
    pub fn from_name(name: &str) -> Option<Kind> {
        Kind::ALL.into_iter().find(|kind| kind.name() == name)
    }

    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Kind::RequestId => "request_id",
            Kind::IpFilter => "ip_filter",
            Kind::Cors => "cors",
            Kind::RateLimit => "rate_limit",
            Kind::BandwidthLimit => "bandwidth_limit",
            Kind::WaitingRoom => "waiting_room",
            Kind::Redirect => "redirect",
            Kind::Respond => "respond",
            Kind::BodyLimit => "body_limit",
            Kind::BasicAuth => "basic_auth",
            Kind::Jwt => "jwt",
            Kind::Oauth2 => "oauth2",
            Kind::ForwardAuth => "forward_auth",
            Kind::BodyRewrite => "body_rewrite",
            Kind::OpenApi => "openapi",
            Kind::Compress => "compress",
            Kind::Headers => "headers",
            Kind::Cache => "cache",
            Kind::StaticFiles => "static_files",
        }
    }

    /// Where the block runs unless it sets `priority`
    #[must_use]
    pub fn default_priority(self) -> u8 {
        match self {
            Kind::RequestId
            | Kind::IpFilter
            | Kind::Cors
            | Kind::RateLimit
            | Kind::BandwidthLimit
            | Kind::WaitingRoom => 0,
            Kind::Redirect | Kind::Respond => 5,
            Kind::BodyLimit => 10,
            Kind::BasicAuth | Kind::Jwt | Kind::Oauth2 | Kind::ForwardAuth => 20,
            // After authentication: bodies of requests that are refused aren't read
            Kind::BodyRewrite => 25,
            // After authentication: who isn't let in learns nothing of the API
            Kind::OpenApi => 22,
            Kind::Compress => 40,
            Kind::Headers => 50,
            Kind::Cache => 60,
            // Last: the other blocks see the request before it's answered
            Kind::StaticFiles => 90,
        }
    }

    /// Whether the block can answer a request instead of the upstream
    #[must_use]
    pub fn answers(self) -> bool {
        matches!(
            self,
            Kind::IpFilter
                | Kind::RateLimit
                | Kind::WaitingRoom
                | Kind::BodyLimit
                | Kind::BasicAuth
                | Kind::Jwt
                | Kind::Oauth2
                | Kind::ForwardAuth
                | Kind::BodyRewrite
                | Kind::OpenApi
        )
    }
}

/// A plugin of a location
pub struct Plugin {
    pub kind: Kind,
    pub priority: u8,
    pub middleware: Arc<dyn Middleware>,
}

/// The plugin `plugin` configures in the location at `location_path`, `None` for names that
/// aren't plugins, or why its configuration is invalid (with the plugin's name in the message)
#[must_use]
pub fn build(plugin: &RoutePlugin, location_path: &str) -> Option<Result<Plugin, String>> {
    let built = |kind, middleware: Result<Arc<dyn Middleware>, String>| {
        Some(middleware.and_then(|middleware| {
            Ok(Plugin {
                kind,
                priority: plugin_priority(kind, plugin)?,
                middleware,
            })
        }))
    };
    // Other plugins of `routes = [...]` and docker labels are ignored, as before
    let kind = Kind::from_name(&plugin.name)?;
    let middleware = match kind {
        Kind::RequestId => arc(RequestId::new(plugin)),
        Kind::IpFilter => arc(IpFilter::new(plugin)),
        Kind::Cors => arc(Cors::new(plugin)),
        Kind::RateLimit => arc(RateLimit::new(plugin).map_err(|err| err.to_string())),
        Kind::BandwidthLimit => arc(BandwidthLimit::new(plugin)),
        Kind::WaitingRoom => arc(WaitingRoom::new(plugin, location_path)),
        Kind::BodyLimit => arc(BodyLimit::new(plugin)),
        Kind::Redirect => arc(Redirect::new(plugin)),
        Kind::Respond => arc(Respond::new(plugin)),
        Kind::BasicAuth => arc(BasicAuth::new(plugin)),
        Kind::Jwt => arc(Jwt::new(plugin)),
        Kind::Oauth2 => arc(Oauth2::new(plugin)),
        Kind::ForwardAuth => arc(ForwardAuth::new(plugin)),
        Kind::BodyRewrite => arc(BodyRewrite::new(plugin)),
        Kind::OpenApi => arc(OpenApi::new(plugin)),
        Kind::Compress => arc(Compress::new(plugin)),
        Kind::StaticFiles => arc(StaticFiles::new(plugin, location_path)),
        // proksi's own blocks, built by proksi
        Kind::Headers | Kind::Cache => return None,
    };
    built(kind, middleware)
}

fn arc<M: Middleware + 'static>(
    middleware: Result<M, String>,
) -> Result<Arc<dyn Middleware>, String> {
    middleware.map(|middleware| Arc::new(middleware) as Arc<dyn Middleware>)
}

#[cfg(test)]
mod tests {
    use super::Kind;

    #[test]
    fn test_kinds_are_listed_once_in_order() {
        // In the order blocks with the same priority run: the order of the enum
        assert!(Kind::ALL.windows(2).all(|pair| pair[0] < pair[1]));
        for kind in Kind::ALL {
            assert_eq!(Kind::from_name(kind.name()), Some(kind));
        }
        assert_eq!(Kind::from_name("unknown"), None);
    }

    #[test]
    fn test_every_plugin_has_settings() {
        let schemas = crate::settings::schemas();
        for kind in Kind::ALL {
            let plugin = !matches!(kind, Kind::Headers | Kind::Cache);
            assert_eq!(schemas.contains_key(kind.name()), plugin, "{}", kind.name());
        }
        // And every set of settings is a plugin's (none missing from `Kind::ALL`)
        assert!(schemas.keys().all(|name| Kind::from_name(name).is_some()));
    }
}

/// What the block `owner` keeps for the current request, if it set something
pub fn block_state<T: Any>(ctx: &mut dyn RequestCtx, owner: usize) -> Option<&mut T> {
    ctx.block_states()
        .iter_mut()
        .find(|(id, state)| *id == owner && state.is::<T>())
        .and_then(|(_, state)| state.downcast_mut())
}

/// Keeps `state` for the block `owner` for the current request, replacing what it kept
pub fn set_block_state<T: Any + Send + Sync>(ctx: &mut dyn RequestCtx, owner: usize, state: T) {
    let states = ctx.block_states();
    states.retain(|(id, kept)| !(*id == owner && kept.is::<T>()));
    states.push((owner, Box::new(state)));
}
