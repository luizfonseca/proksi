use std::time::{Duration, SystemTime};
use std::{borrow::Cow, collections::HashMap, sync::Arc};

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

use pingora_cache::{
    CacheKey, CacheMeta, ForcedFreshness, HitHandler, NoCacheReason, RespCacheable,
};

use crate::config::RouteUpstream;
use crate::metrics::{LocationMetrics, UpstreamMetrics};
use crate::stores::{self, routes::RouteStoreContainer};

use super::{default_peer_opts, http_proxy, listener::Listener};

static CACHEABLE_METHODS: Lazy<Vec<http::Method>> =
    Lazy::new(|| vec![http::Method::GET, http::Method::HEAD]);

/// Proxies requests to the location of their host, on the HTTPS or the HTTP port
pub struct Router {
    pub listener: Listener,
    /// Where hosts without `listen` addresses are served for this protocol
    pub default_address: std::net::SocketAddr,
}

// type Container = mapref::one::Ref<'static, String, RouteStoreContainer>;

// fn process_route(ctx: &RouterContext) -> RouteStoreContainer {
//     ctx.route_container.clone()
// }

pub struct RouterContext {
    pub host: String,
    /// The location serving the request, once found
    pub route_container: Option<Arc<RouteStoreContainer>>,
    pub upstream: RouteUpstream,
    pub extensions: HashMap<Cow<'static, str>, String>,
    /// Bytes of the request body proxied so far
    pub request_body_bytes: u64,
    /// The upstream chosen (index in the location's upstreams), and when
    pub upstream_index: Option<usize>,
    pub upstream_start: Option<std::time::Instant>,

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
            route_container: None,
            upstream: RouteUpstream::default(),
            extensions: HashMap::with_capacity(2),
            request_body_bytes: 0,
            upstream_index: None,
            upstream_start: None,

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
        let req_host = get_host(session).to_string();
        let host_without_port = req_host.split(':').next().unwrap_or_default();
        host_without_port.clone_into(&mut ctx.host);

        // The host with that name (case-insensitive), or `*`, among the hosts served
        // on the address the request came in on
        let local = session
            .server_addr()
            .and_then(|addr| addr.as_inet())
            .copied();
        let serves_host = |host: &std::sync::Arc<stores::host_container::HostContainer>| {
            let protocol = match self.listener {
                Listener::Https => host.tls_enabled,
                Listener::Http => !host.tls_enabled,
            };
            protocol && host.listens_on(local.as_ref(), &self.default_address)
        };
        let host_container = stores::lookup::find_host(host_without_port, serves_host);

        if self.listener == Listener::Http {
            if req_host.is_empty() {
                return Err(pingora::Error::new(HTTPStatus(400)));
            }
            // `/ping` is proksi's own health check, whatever the host
            if http_proxy::serve_ping(session).await? {
                return Ok(true);
            }
            // Hosts with TLS (and unknown hosts): ACME challenges, then redirect to HTTPS.
            // Hosts without TLS get no certificates, so all their paths are proxied.
            if host_container.is_none() {
                if http_proxy::serve_acme_challenge(session, &req_host).await? {
                    return Ok(true);
                }
                let port = stores::get_route_by_key(&host_without_port.to_lowercase())
                    .and_then(|host| host.https_redirect_port());
                return http_proxy::redirect_to_https(session, &req_host, port).await;
            }
        }

        // If there's no host matching, returns a 404
        let Some(host_container) = host_container else {
            session.respond_error(404).await?;
            return Ok(true);
        };

        // The most specific location of the host for the path, or a 404
        let uri = get_uri(session);
        let Some(route_container) = host_container.location_for(uri.path()) else {
            session.respond_error(404).await?;
            return Ok(true);
        };

        // Set first, so requests answered by a block are counted in the location's metrics
        ctx.route_container = Some(route_container.clone());

        // The location's building blocks, in order: a block can answer the request (e.g. a `401`)
        if let Some(block) = route_container.blocks.request(session, ctx).await {
            if let Some(metrics) = &route_container.metrics {
                metrics.blocked(block.name());
            }
            return Ok(true);
        }

        Ok(false)
    }

    /// Each part of the request body, as it's proxied to the upstream
    async fn request_body_filter(
        &self,
        _session: &mut Session,
        body: &mut Option<bytes::Bytes>,
        _end_of_stream: bool,
        ctx: &mut Self::CTX,
    ) -> pingora::Result<()> {
        let Some(location) = ctx.route_container.clone() else {
            return Ok(());
        };
        location
            .blocks
            .request_body(body.as_ref(), ctx)
            .map_err(|(block, err)| {
                if let Some(metrics) = &location.metrics {
                    metrics.blocked(block.name());
                }
                err
            })
    }

    /// Define where the proxy should send the request to.
    ///
    /// The returned [`HttpPeer`] contains the information regarding
    /// where and how this request should forwarded to."]
    async fn upstream_peer(
        &self,
        session: &mut Session,
        ctx: &mut Self::CTX,
    ) -> pingora::Result<Box<HttpPeer>> {
        // If there's no host matching, returns a 404
        let Some(route_container) = ctx.route_container.clone() else {
            return Err(pingora::Error::new(HTTPStatus(404)));
        };

        if session.cache.enabled() {
            session.cache.set_max_file_size_bytes(100 * 1024 * 1024);
        }

        let Some(healthy_upstream) = route_container.load_balancer.select(b"", 32) else {
            return Err(pingora::Error::new(HTTPStatus(503)));
        };

        let Some(healthy_addr) = healthy_upstream.addr.as_inet() else {
            return Err(pingora::Error::new(HTTPStatus(503)));
        };
        let healthy_port = healthy_addr.port();

        let Some((index, upstream)) = route_container.upstream(&healthy_upstream) else {
            return Err(pingora::Error::new(HTTPStatus(503)));
        };

        ctx.upstream = upstream.clone();
        ctx.upstream_index = Some(index);
        ctx.upstream_start = Some(std::time::Instant::now());

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
        if let Some(location) = ctx.route_container.clone() {
            location
                .blocks
                .response(session, upstream_response, ctx)
                .await;
        }

        let cache_state = ctx.extensions.get("cache_state").cloned();
        if let Some(cache_state) = cache_state.filter(|_| session.cache.enabled()) {
            // indicates whether it was HIT or MISS in the cache
            upstream_response.insert_header(
                HeaderName::from_static("cache-status"),
                cache_state.as_str(),
            )?;

            let elapsed = ctx.timings.request_filter_start.elapsed();
            upstream_response.insert_header(
                HeaderName::from_static("cache-duration"),
                elapsed.as_millis().to_string(),
            )?;
        }

        Ok(())
    }

    /// Modify the request before it is sent to the upstream
    ///
    /// Unlike [`Self::request_filter()`], this filter allows to change the request headers to send
    /// to the upstream.
    async fn upstream_request_filter(
        &self,
        session: &mut Session,
        upstream_request: &mut RequestHeader,
        ctx: &mut Self::CTX,
    ) -> pingora::Result<()> {
        // Who the client is, set first so the location's headers can still change them
        if let Some(peer) = session
            .client_addr()
            .and_then(|addr| addr.as_inet())
            .map(std::net::SocketAddr::ip)
        {
            let header = |name: &str| {
                session
                    .req_header()
                    .headers
                    .get(name)
                    .and_then(|value| value.to_str().ok())
            };
            let proto = match self.listener {
                Listener::Https => "https",
                Listener::Http => "http",
            };
            let forwarded = crate::proxy_server::client::forwarded(
                peer,
                crate::proxy_server::client::Incoming {
                    forwarded_for: header("x-forwarded-for"),
                    proto: header("x-forwarded-proto"),
                    host: header("x-forwarded-host"),
                },
                proto,
                header("host").or_else(|| session.req_header().uri.host()),
            );
            upstream_request.insert_header("x-forwarded-for", forwarded.forwarded_for)?;
            upstream_request.insert_header("x-forwarded-proto", forwarded.proto)?;
            if let Some(host) = forwarded.host {
                upstream_request.insert_header("x-forwarded-host", host)?;
            }
            upstream_request.insert_header("x-real-ip", forwarded.real_ip)?;
        }

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

        if let Some(location) = ctx.route_container.clone() {
            location
                .blocks
                .upstream_request(session, upstream_request, ctx)
                .await;
        }

        Ok(())
    }

    /// Modify the response header from the upstream
    ///
    /// The modification is before caching, so any change here will be stored in the cache if enabled.
    ///
    /// Responses served from cache won't trigger this filter. If the cache needed revalidation,
    /// only the 304 from upstream will trigger the filter (though it will be merged into the
    /// cached header, not served directly to downstream).
    async fn upstream_response_filter(
        &self,
        session: &mut Session,
        upstream_response: &mut ResponseHeader,
        ctx: &mut Self::CTX,
    ) -> pingora::Result<()> {
        // If there's no host matching, returns a 404
        // let route_container = process_route(ctx);

        if let Some(location) = ctx.route_container.clone() {
            if let (Some(upstream), Some(start)) = (upstream_metrics(ctx), ctx.upstream_start) {
                upstream.record(start.elapsed());
            }
            location
                .blocks
                .upstream_response(session, upstream_response, ctx);
        }

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
        let duration = ctx.timings.request_filter_start.elapsed();
        let duration_ms = duration.as_millis();

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

        // The connection's address, or behind trusted proxies, the client they forwarded
        let peer = session.client_addr();
        let forwarded_for = session
            .req_header()
            .headers
            .get("x-forwarded-for")
            .and_then(|value| value.to_str().ok());
        let client_ip = match peer.and_then(|addr| addr.as_inet()) {
            Some(addr) => {
                let client = crate::proxy_server::client::client_ip(addr.ip(), forwarded_for);
                if client == addr.ip() {
                    addr.to_string()
                } else {
                    client.to_string()
                }
            }
            None => peer.map(ToString::to_string).unwrap_or_default(),
        };

        let status_code = session
            .response_written()
            .map(|v| v.status.as_u16())
            .unwrap_or_default();

        // Requests no location served (unknown hosts, redirects to HTTPS...) are `_unknown`
        if crate::metrics::enabled() {
            ctx.route_container
                .as_ref()
                .and_then(|location| location.metrics.as_ref())
                .unwrap_or_else(|| LocationMetrics::unknown())
                .record(status_code, duration);
        }

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
        let path_and_query = session
            .req_header()
            .uri
            .path_and_query()
            .map_or("/", PathAndQuery::as_str);

        Ok(cache_key_for(&ctx.host, path_and_query))
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
        _session: &mut Session,
        meta: &CacheMeta,
        _hit_handler: &mut HitHandler,
        _is_fresh: bool,
        ctx: &mut Self::CTX,
    ) -> pingora::Result<Option<ForcedFreshness>> {
        if !meta.is_fresh(SystemTime::now()) {
            ctx.extensions
                .insert(Cow::Borrowed("cache_state"), "expired".into());
            // Keeps the asset's stale-while-revalidate/stale-if-error windows open,
            // matching the pre-0.9 `ForceExpired` behavior
            return Ok(Some(ForcedFreshness::ForceExpiredServeStale {
                expired_at: None,
            }));
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
        let Some(cache) = ctx.route_container.as_ref().and_then(|c| c.cache.as_ref()) else {
            return Ok(RespCacheable::Uncacheable(NoCacheReason::NeverEnabled));
        };

        // Only cache GET and HEAD requests with 2xx responses
        if !CACHEABLE_METHODS.contains(&session.req_header().method) {
            return Ok(RespCacheable::Uncacheable(NoCacheReason::Custom(
                "method or status not cacheable",
            )));
        }

        let now = SystemTime::now();
        let Some(fresh_until) = now.checked_add(Duration::from_secs(cache.expires_in_secs)) else {
            return Ok(RespCacheable::Uncacheable(NoCacheReason::Custom(
                "cache expiration out of range",
            )));
        };

        Ok(RespCacheable::Cacheable(CacheMeta::new(
            fresh_until,
            now,
            cache.stale_while_revalidate_secs,
            cache.stale_if_error_secs,
            resp.clone(),
        )))
    }

    /// This filter is called when the request just established or reused a connection to the upstream
    ///
    /// This filter allows user to log timing and connection related info.
    fn fail_to_connect(
        &self,
        _session: &mut Session,
        _peer: &HttpPeer,
        ctx: &mut Self::CTX,
        e: Box<pingora::Error>,
    ) -> Box<pingora::Error> {
        if let Some(upstream) = upstream_metrics(ctx) {
            upstream.error();
        }
        // The upstream's name may point elsewhere now (ADR 0006)
        if let Some(route) = &ctx.route_container {
            let load_balancer = route.load_balancer.clone();
            tokio::spawn(async move { load_balancer.update().await });
        }
        e
    }

    fn error_while_proxy(
        &self,
        peer: &HttpPeer,
        session: &mut Session,
        e: Box<pingora::Error>,
        ctx: &mut Self::CTX,
        client_reused: bool,
    ) -> Box<pingora::Error> {
        if let Some(upstream) = upstream_metrics(ctx) {
            upstream.error();
        }
        // Pingora's default
        let mut e = e.more_context(format!("Peer: {peer}"));
        if !session.req_header().method.is_idempotent() || session.as_ref().retry_buffer_truncated()
        {
            e.set_retry(false);
        } else {
            e.retry.decide_reuse(client_reused);
        }
        e
    }

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

/// The metrics of the upstream chosen for the request
fn upstream_metrics(ctx: &RouterContext) -> Option<&UpstreamMetrics> {
    let location = ctx.route_container.as_ref()?.metrics.as_ref()?;
    location.upstream(ctx.upstream_index?)
}

/// Builds the cache key for a request to `host`.
///
/// Pingora 0.9 dropped the separate cache key namespace. Prefixing the host keeps the same hash
/// as `namespace + primary` did before, so existing cache entries stay valid (the base64 of a
/// path always starts with `L` and is never followed by another `L`, so the boundary is
/// unambiguous). The host is kept in `user_tag` for the disk cache directory lookup.
fn cache_key_for(host: &str, path_and_query: &str) -> CacheKey {
    let primary = format!("{host}{}", base64::encode_block(path_and_query.as_bytes()));
    CacheKey::new(primary, host)
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

#[cfg(test)]
mod tests {
    use pingora::proxy::ProxyHttp;
    use pingora_cache::key::CacheHashKey;

    use super::{cache_key_for, Listener, Router};

    #[test]
    fn test_cache_key_matches_pre_pingora_0_9_hash() {
        // Hash produced by pingora-cache 0.5 for
        // `CacheKey::new("example.com", base64("/assets/app.js?v=1"), "")`
        let key = cache_key_for("example.com", "/assets/app.js?v=1");

        assert_eq!(key.primary(), "6d8d9b6395c298da5763bb93958a01c0");
        assert_eq!(key.user_tag, "example.com");
    }

    #[test]
    fn test_new_context_has_no_location() {
        let router = Router {
            listener: Listener::Http,
            default_address: "0.0.0.0:80".parse().unwrap(),
        };

        // Created for every request: the location is only shared once it's found
        assert!(router.new_ctx().route_container.is_none());
    }
}
