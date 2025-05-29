use std::net::ToSocketAddrs;
use std::str::FromStr;
use std::time::{Duration, SystemTime};
use std::{borrow::Cow, collections::HashMap};

use async_trait::async_trait;

use http::uri::PathAndQuery;
use http::{HeaderName, HeaderValue, Uri};
use once_cell::sync::Lazy;

use openssl::base64;
use pingora::http::{RequestHeader, ResponseHeader};
use pingora::protocols::Digest;
use pingora::proxy::{ProxyHttp, Session};
use pingora::upstreams::peer::Peer;
use pingora::{upstreams::peer::HttpPeer, ErrorType::HTTPStatus};

use pingora_cache::lock::CacheLock;

use pingora_cache::{CacheKey, CacheMeta, ForcedInvalidationKind, NoCacheReason, RespCacheable};

use crate::cache::disk::storage::DiskCache;
use crate::config::{RouteCacheType, RouteUpstream};
use crate::stores::{self, routes::RouteStoreContainer};

use super::default_peer_opts;
use super::middleware::{
    execute_request_plugins, execute_response_plugins, execute_upstream_request_plugins,
    execute_upstream_response_plugins,
};

static STORAGE_MEM_CACHE: Lazy<pingora_cache::MemCache> = Lazy::new(pingora_cache::MemCache::new);
static STORAGE_CACHE: Lazy<DiskCache> = Lazy::new(DiskCache::new);
static CACHEABLE_METHODS: Lazy<Vec<http::Method>> =
    Lazy::new(|| vec![http::Method::GET, http::Method::HEAD]);
static CACHE_LOCK: Lazy<CacheLock> = Lazy::new(|| CacheLock::new(Duration::from_secs(1)));

/// Load balancer proxy struct
pub struct Router {}

// type Container = mapref::one::Ref<'static, String, RouteStoreContainer>;

// fn process_route(ctx: &RouterContext) -> RouteStoreContainer {
//     ctx.route_container.clone()
// }

fn get_cache_storage(cache_type: &RouteCacheType) -> &'static (dyn pingora_cache::Storage + Sync) {
    match cache_type {
        RouteCacheType::Disk => &*STORAGE_CACHE,
        RouteCacheType::MemCache => &*STORAGE_MEM_CACHE,
    }
}

pub struct RouterContext {
    pub host: String,
    pub route_container: RouteStoreContainer,
    pub upstream: RouteUpstream,
    pub extensions: HashMap<Cow<'static, str>, String>,

    pub timings: RouterTimings,
}

pub struct RouterTimings {
    request_filter_start: std::time::Instant,
}

#[async_trait]
impl ProxyHttp for Router {
    /// The per request object to share state across the different filters
    type CTX = RouterContext;

    /// Define how the `ctx` should be created.
    fn new_ctx(&self) -> Self::CTX {
        RouterContext {
            host: String::new(),
            route_container: RouteStoreContainer::default(),
            upstream: RouteUpstream::default(),
            extensions: HashMap::with_capacity(2),

            timings: RouterTimings {
                request_filter_start: std::time::Instant::now(),
            },
        }
    }

    // Define the filter that will be executed before the request is sent to the upstream.
    // If the filter returns `true`, the request has already been handled.
    // If the filter returns `false`, the request will be sent to the upstream.
    async fn request_filter(
        &self,
        session: &mut Session,
        ctx: &mut Self::CTX,
    ) -> pingora::Result<bool> {
        let req_host = get_host(session);
        let host_without_port = req_host.split(':').collect::<Vec<_>>()[0];
        host_without_port.clone_into(&mut ctx.host);

        ctx.host = host_without_port.to_string();

        // If there's no host matching, returns a 404
        let Some(route_container) = stores::get_route_by_key(host_without_port) else {
            session.respond_error(404).await?;
            return Ok(true);
        };

        // Match request pattern based on the URI
        let uri = get_uri(session);

        match &route_container.path_matcher.pattern {
            Some(pattern) if pattern.find(uri.path()).is_none() => {
                session.respond_error(404).await?;
                return Ok(true);
            }
            _ => {}
        }

        // Middleware phase: request_filterx
        // We are checking to see if the request has already been handled
        // by the plugins i.e. (ok(true))
        if let Ok(true) = execute_request_plugins(session, ctx, &route_container.plugins).await {
            return Ok(true);
        }

        if route_container.cache.is_some() {
            let cache = route_container.cache.as_ref().unwrap();
            if cache.enabled.unwrap_or(false) {
                let storage = get_cache_storage(&cache.cache_type);

                stores::insert_cache_routing(
                    &ctx.host,
                    cache.path.to_string_lossy().to_string(),
                    false,
                );
                session
                    .cache
                    .enable(storage, None, None, Some(&*CACHE_LOCK));
            }
        }

        ctx.route_container = route_container.clone();

        Ok(false)
    }

    /// Define where the proxy should send the request to.
    ///
    /// The returned [HttpPeer] contains the information regarding
    /// where and how this request should forwarded to."]
    async fn upstream_peer(
        &self,
        session: &mut Session,
        ctx: &mut Self::CTX,
    ) -> pingora::Result<Box<HttpPeer>> {
        // If there's no host matching, returns a 404
        let route_container = &ctx.route_container;

        if session.cache.enabled() {
            session.cache.set_max_file_size_bytes(100 * 1024 * 1024);
        }

        let Some(healthy_upstream) = route_container.load_balancer.select(b"", 32) else {
            return Err(pingora::Error::new(HTTPStatus(503)));
        };

        let (healthy_ip, healthy_port) = if let Some(scr) = healthy_upstream.addr.as_inet() {
            (scr.ip().to_string(), scr.port())
        } else {
            return Err(pingora::Error::new(HTTPStatus(503)));
        };

        let Some(upstream) = route_container.upstreams.iter().find(|u| {
            format!("{}:{}", u.ip, u.port)
                .to_socket_addrs()
                .unwrap()
                .any(|s| s.ip().to_string() == healthy_ip && s.port() == healthy_port)
        }) else {
            return Err(pingora::Error::new(HTTPStatus(503)));
        };

        ctx.upstream = upstream.clone();

        // https://github.com/cloudflare/pingora/blob/main/docs/user_guide/peer.md?plain=1#L17
        let mut peer = HttpPeer::new(
            healthy_upstream,
            healthy_port == 443,
            upstream.sni.clone().unwrap_or(String::new()),
        );
        peer.options = default_peer_opts();
        Ok(Box::new(peer))
    }

    /// Modify the response header before it is send to the downstream
    ///
    /// The modification is after caching. This filter is called for all responses including
    /// responses served from cache.
    async fn response_filter(
        &self,
        session: &mut Session,
        upstream_response: &mut ResponseHeader,
        ctx: &mut Self::CTX,
    ) -> pingora::Result<()> {
        // If there's no host matching, returns a 404
        let route_container = &ctx.route_container;

        for (name, value) in &route_container.host_header_add {
            upstream_response.insert_header(name, value)?;
        }

        // Remove headers from the upstream response
        for name in &route_container.host_header_remove {
            upstream_response.remove_header(name);
        }

        let cache_state = ctx.extensions.get("cache_state").cloned();
        if session.cache.enabled() && cache_state.is_some() {
            let cache_state = cache_state.unwrap();
            // indicates whether it was HIT or MISS in the cache
            upstream_response.insert_header(
                HeaderName::from_str("cache-status").unwrap(),
                cache_state.as_str(),
            )?;

            let elapsed = ctx.timings.request_filter_start.elapsed();
            upstream_response.insert_header(
                HeaderName::from_str("cache-duration").unwrap(),
                elapsed.as_millis().to_string(),
            )?;
        }

        // Middleware phase: response_filterx
        execute_response_plugins(session, ctx).await?;

        Ok(())
    }

    /// Modify the request before it is sent to the upstream
    ///
    /// Unlike [Self::request_filter()], this filter allows to change the request headers to send
    /// to the upstream.
    async fn upstream_request_filter(
        &self,
        session: &mut Session,
        upstream_request: &mut RequestHeader,
        ctx: &mut Self::CTX,
    ) -> pingora::Result<()> {
        // If there's no host matching, returns a 404
        // let route_container = &ctx.route_container;

        let upstream = &ctx.upstream;

        // TODO: refactor
        if let Some(headers) = upstream.headers.as_ref() {
            if let Some(add) = headers.add.as_ref() {
                for header_add in add {
                    upstream_request
                        .insert_header(header_add.name.to_string(), header_add.value.to_string())
                        .ok();
                }
            }
        }

        execute_upstream_request_plugins(session, upstream_request, ctx)
            .await
            .ok();

        Ok(())
    }

    /// Modify the response header from the upstream
    ///
    /// The modification is before caching, so any change here will be stored in the cache if enabled.
    ///
    /// Responses served from cache won't trigger this filter. If the cache needed revalidation,
    /// only the 304 from upstream will trigger the filter (though it will be merged into the
    /// cached header, not served directly to downstream).
    fn upstream_response_filter(
        &self,
        session: &mut Session,
        upstream_response: &mut ResponseHeader,
        ctx: &mut Self::CTX,
    ) -> Result<(), Box<pingora::Error>> {
        // If there's no host matching, returns a 404
        // let route_container = process_route(ctx);

        execute_upstream_response_plugins(session, upstream_response, ctx);

        Ok(())
    }

    /// This filter is called when the entire response is sent to the downstream successfully or
    /// there is a fatal error that terminate the request.
    ///
    /// An error log is already emitted if there is any error. This phase is used for collecting
    /// metrics and sending access logs.
    async fn logging(
        &self,
        session: &mut Session,
        _: Option<&pingora::Error>,
        ctx: &mut Self::CTX,
    ) {
        let duration_ms = ctx.timings.request_filter_start.elapsed().as_millis();

        let http_version = if session.is_http2() {
            "http/2"
        } else {
            "http/1.1"
        };

        let method = session.req_header().method.to_string();
        let query = session.req_header().uri.query().unwrap_or_default();
        let path = session.req_header().uri.path();
        let empty_header = HeaderValue::from_static("");
        let host = session.req_header().uri.host();
        let referer = session
            .req_header()
            .headers
            .get("referer")
            .unwrap_or(&empty_header);

        let user_agent = session
            .req_header()
            .headers
            .get("user-agent")
            .unwrap_or(&empty_header);

        let client_ip = session
            .client_addr()
            .map(ToString::to_string)
            .unwrap_or_default();

        let status_code = session
            .response_written()
            .map(|v| v.status.as_u16())
            .unwrap_or_default();

        tracing::info!(
            method,
            path,
            query,
            host,
            duration_ms,
            user_agent = user_agent.to_str().unwrap_or(""),
            referer = referer.to_str().unwrap_or(""),
            client_ip,
            status_code,
            http_version,
            reused_connection = ctx.extensions.get("reused").unwrap_or(&String::new()),
            peer_addr = ctx.extensions.get("peer").unwrap_or(&String::new()),
            request_id = ctx.extensions.get("request_id_header"),
            access_log = true
        );
    }

    // This callback generates the cache key
    ///
    /// This callback is called only when cache is enabled for this request
    ///
    /// By default this callback returns a default cache key generated from the request.
    fn cache_key_callback(
        &self,
        session: &Session,
        ctx: &mut Self::CTX,
    ) -> pingora::Result<CacheKey> {
        let req_header = session.req_header();
        Ok(CacheKey::new(
            ctx.host.clone(),
            base64::encode_block(
                req_header
                    .uri
                    .path_and_query()
                    .unwrap_or(&PathAndQuery::from_static("/"))
                    .as_str()
                    .as_bytes(),
            ),
            "",
        ))
    }

    /// This callback is invoked when a cacheable response is ready to be admitted to cache
    fn cache_miss(&self, session: &mut Session, ctx: &mut Self::CTX) {
        ctx.extensions
            .insert(Cow::Borrowed("cache_state"), "fwd=miss".into());
        session.cache.cache_miss();
    }

    /// This filter is called after a successful cache lookup and before the cache asset is ready to
    /// be used.
    ///
    /// This filter allow the user to log or force expire the asset.
    // flex purge, other filtering, returns whether asset is should be force expired or not
    async fn cache_hit_filter(
        &self,
        _session: &Session,
        meta: &CacheMeta,
        _enabled: bool,
        ctx: &mut Self::CTX,
    ) -> pingora::Result<Option<ForcedInvalidationKind>> {
        if !meta.is_fresh(SystemTime::now()) {
            ctx.extensions
                .insert(Cow::Borrowed("cache_state"), "expired".into());
            return Ok(Some(ForcedInvalidationKind::ForceExpired));
        }

        ctx.extensions
            .insert(Cow::Borrowed("cache_state"), "hit".into());
        Ok(None)
    }

    /// Decide if the response is cacheable
    fn response_cache_filter(
        &self,
        session: &Session,
        resp: &ResponseHeader,
        ctx: &mut Self::CTX,
    ) -> pingora::Result<RespCacheable> {
        let container = &ctx.route_container;
        let Some(cache) = container.cache.as_ref() else {
            return Ok(RespCacheable::Uncacheable(NoCacheReason::NeverEnabled));
        };

        // Only cache GET and HEAD requests with 2xx responses
        if !CACHEABLE_METHODS.contains(&session.req_header().method) {
            return Ok(RespCacheable::Uncacheable(NoCacheReason::Custom(
                "method or status not cacheable",
            )));
        }

        Ok(RespCacheable::Cacheable(CacheMeta::new(
            SystemTime::now()
                .checked_add(Duration::from_secs(cache.expires_in_secs))
                .unwrap(),
            SystemTime::now(),
            cache.stale_while_revalidate_secs,
            cache.stale_if_error_secs,
            resp.clone(),
        )))
    }

    /// This filter is called when the request just established or reused a connection to the upstream
    ///
    /// This filter allows user to log timing and connection related info.
    async fn connected_to_upstream(
        &self,
        _session: &mut Session,
        reused: bool,
        peer: &HttpPeer,
        _fd: std::os::unix::io::RawFd,
        _digest: Option<&Digest>,
        ctx: &mut Self::CTX,
    ) -> pingora::Result<()>
    where
        Self::CTX: Send + Sync,
    {
        ctx.extensions
            .insert(Cow::Borrowed("reused"), reused.to_string());
        ctx.extensions
            .insert(Cow::Borrowed("peer"), peer.address().to_string());
        Ok(())
    }
}

fn get_uri(session: &mut Session) -> Uri {
    session.req_header().uri.clone()
}

/// Retrieves the host from the request headers based on
/// whether the request is HTTP/1.1 or HTTP/2
fn get_host(session: &mut Session) -> &str {
    if let Some(host) = session.get_header(http::header::HOST) {
        return host.to_str().unwrap_or("");
    }

    if let Some(host) = session.req_header().uri.host() {
        return host;
    }

    ""
}
