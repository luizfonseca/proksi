use std::time::{Duration, SystemTime};
use std::sync::Arc;

use async_trait::async_trait;

use http::uri::PathAndQuery;
use http::{HeaderName, HeaderValue};
use once_cell::sync::Lazy;

use openssl::base64;
use pingora::http::{RequestHeader, ResponseHeader};
use pingora::protocols::Digest;
use pingora::proxy::{FailToProxy, ProxyHttp, Session};
use pingora::upstreams::peer::Peer;
use pingora::{upstreams::peer::HttpPeer, ErrorType::HTTPStatus};

use pingora_cache::{
    CacheKey, CacheMeta, ForcedFreshness, HitHandler, NoCacheReason, RespCacheable,
};

use crate::metrics::{LocationMetrics, UpstreamMetrics};
use crate::stores::{self, routes::RouteStoreContainer};

use super::{default_peer_opts, http_proxy, listener::Listener, respond::respond_error};

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
    /// The request's id (`request_id` block)
    pub request_id: Option<http::HeaderValue>,
    /// Whether the response came from the cache (`cache-status`)
    pub cache_state: Option<&'static str>,
    /// The connection to the upstream: whether it was reused, and its address
    pub upstream_connection: Option<(bool, pingora::protocols::l4::socket::SocketAddr)>,
    /// The rules the request matched, whose blocks take part in its later stages
    pub rules: crate::rules::Applied,
    /// Bytes of the request body proxied so far
    pub request_body_bytes: u64,
    /// The upstream chosen (index in the location's upstreams), and when
    pub upstream_index: Option<usize>,
    pub upstream_start: Option<std::time::Instant>,
    /// When the request body must have arrived (`body_secs`), and the timeout of each read
    pub body_deadline: Option<(std::time::Instant, Duration)>,
    /// A health check (`/ping`): neither logged nor counted
    pub health_check: bool,
    /// The traffic split a rule chose (`then { traffic_split = "..." }`)
    pub traffic_split: Option<Arc<str>>,
    /// Traffic split cookies to set on the response: name, and the new client's id
    pub split_cookies: Vec<(String, String)>,
    /// Headers blocks add to the response (e.g. a waiting room's cookie)
    pub response_headers: Vec<(http::HeaderName, http::HeaderValue)>,
    /// Whether a rule chose to copy the request to the location's mirror, or not to
    pub mirror_rule: Option<bool>,
    /// The copy of the request for the location's mirror
    pub mirror: crate::routing::mirror::State,
    /// Times the request was sent to an upstream, and the upstreams it was sent to
    pub tries: u8,
    pub tried: crate::routing::balance::Tried,
    /// Whether a rule allows retries (`then { retry = false }`)
    pub retry_rule: Option<bool>,
    /// The requests in progress to the upstream chosen, counted (least connections)
    pub in_flight: Option<Arc<std::sync::atomic::AtomicUsize>>,

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
            request_id: None,
            cache_state: None,
            upstream_connection: None,
            rules: crate::rules::Applied::default(),
            request_body_bytes: 0,
            upstream_index: None,
            upstream_start: None,
            body_deadline: None,
            health_check: false,
            traffic_split: None,
            split_cookies: vec![],
            response_headers: vec![],
            mirror_rule: None,
            mirror: crate::routing::mirror::State::default(),
            tries: 0,
            tried: crate::routing::balance::Tried::default(),
            retry_rule: None,
            in_flight: None,

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
        get_host(session)
            .split(':')
            .next()
            .unwrap_or_default()
            .clone_into(&mut ctx.host);

        // The server's rules, before the host is chosen (ADR 0007)
        if let Some(rules) = crate::rules::server() {
            if rules.request(session, ctx).await {
                return Ok(true);
            }
        }
        let host_without_port = ctx.host.as_str();

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
        let mut host_container = stores::lookup::find_host(host_without_port, serves_host);

        if self.listener == Listener::Http {
            if get_host(session).is_empty() {
                return Err(pingora::Error::new(HTTPStatus(400)));
            }
            // `/ping` is proksi's own health check, whatever the host
            if http_proxy::serve_ping(session).await? {
                ctx.health_check = true;
                return Ok(true);
            }
            // Hosts with TLS (and unknown hosts): ACME challenges, then redirect to HTTPS.
            // Hosts without TLS get no certificates, so all their paths are proxied.
            if host_container.is_none() {
                let Some(found) = acme_or_redirect(session, ctx).await? else {
                    return Ok(true);
                };
                host_container = Some(found);
            }
        }

        // If there's no host matching, returns a 404
        let Some(host_container) = host_container else {
            respond_error(session, ctx, 404).await?;
            return Ok(true);
        };

        // The host's rules, before its location is chosen
        if let Some(rules) = &host_container.rules {
            if rules.request(session, ctx).await {
                return Ok(true);
            }
        }

        // The most specific location of the host for the path, or a 404
        let Some(route_container) = host_container.location_for(session.req_header().uri.path())
        else {
            respond_error(session, ctx, 404).await?;
            return Ok(true);
        };

        // Set first, so requests answered by a block are counted in the location's metrics
        ctx.route_container = Some(route_container.clone());

        // The location's timeouts, instead of `server.timeouts`
        if let Some(timeouts) = route_container.timeouts {
            if let Some(read) = timeouts.read_secs {
                session.set_read_timeout(Some(Duration::from_secs(read)));
            }
            if let Some(write) = timeouts.write_secs {
                session.set_write_timeout(Some(Duration::from_secs(write)));
            }
        }
        // The whole body, from now: each read waits until then at most
        let server = super::timeouts::client_timeouts();
        let timeouts = route_container.timeouts.unwrap_or_default();
        if let Some(body) = timeouts.body_secs.or(server.body_secs) {
            let read = Duration::from_secs(timeouts.read_secs.unwrap_or(server.read_secs));
            let body = Duration::from_secs(body);
            ctx.body_deadline = Some((std::time::Instant::now() + body, read));
            session.set_read_timeout(Some(read.min(body)));
        }

        // The location's building blocks, in order: a block can answer the request (e.g. a `401`)
        // The location's rules, then its blocks
        if let Some(rules) = &route_container.rules {
            if rules.request(session, ctx).await {
                return Ok(true);
            }
        }
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
        session: &mut Session,
        body: &mut Option<bytes::Bytes>,
        end_of_stream: bool,
        ctx: &mut Self::CTX,
    ) -> pingora::Result<()> {
        // The rest of the body must arrive by the deadline (HTTP/2: checked as parts arrive)
        if let Some((deadline, read)) = ctx.body_deadline.filter(|_| !end_of_stream) {
            let remaining = deadline.saturating_duration_since(std::time::Instant::now());
            if remaining.is_zero() {
                return Err(pingora::Error::explain(
                    pingora::ErrorType::ReadTimedout,
                    "the request body didn't arrive in time (body_secs)",
                )
                .into_down());
            }
            session.set_read_timeout(Some(read.min(remaining)));
        }

        crate::rules::request_body(body.as_ref(), ctx)?;
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
            })?;
        // The body of the mirror's copy, as proxied
        crate::routing::mirror::body(&mut ctx.mirror, body.as_ref(), end_of_stream);
        Ok(())
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

        // Tried again: the upstream tried last no longer counts this request
        release_in_flight(ctx);
        ctx.tries = ctx.tries.saturating_add(1);

        let Some(picked) = crate::routing::split::select(session, ctx, &route_container).await
        else {
            return Err(pingora::Error::new(HTTPStatus(503)));
        };
        let least =
            route_container.load_balancing == crate::config::host::LoadBalancing::LeastConnections;
        if let Some(count) = picked.in_flight.filter(|_| least) {
            count.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            ctx.in_flight = Some(count);
        }

        let Some((index, upstream)) = picked
            .index
            .and_then(|index| Some((index, route_container.upstreams.get(index)?)))
        else {
            return Err(pingora::Error::new(HTTPStatus(503)));
        };

        ctx.upstream_index = Some(index);
        ctx.tried.add(index);
        ctx.upstream_start = Some(std::time::Instant::now());

        // https://github.com/cloudflare/pingora/blob/main/docs/user_guide/peer.md?plain=1#L17
        let mut peer = HttpPeer::new(
            picked.addr,
            picked.addr.port() == 443,
            upstream.sni.clone().unwrap_or_default(),
        );
        let timeouts = route_container
            .timeouts
            .and_then(|timeouts| timeouts.upstream)
            .unwrap_or_default()
            .or(Some(super::timeouts::upstream_timeouts()));
        peer.options = default_peer_opts(timeouts);
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
        crate::rules::response(session, upstream_response, ctx).await;

        for (name, value) in std::mem::take(&mut ctx.response_headers) {
            upstream_response.append_header(name, value)?;
        }

        // A new client of a traffic split keeps its side
        for (name, id) in std::mem::take(&mut ctx.split_cookies) {
            let secure = if self.listener == Listener::Https { "; Secure" } else { "" };
            upstream_response.append_header(
                http::header::SET_COOKIE,
                format!("{name}={id}; Path=/; Max-Age=2592000; HttpOnly; SameSite=Lax{secure}"),
            )?;
        }

        if let Some(cache_state) = ctx.cache_state.filter(|_| session.cache.enabled()) {
            // indicates whether it was HIT or MISS in the cache
            upstream_response.insert_header(HeaderName::from_static("cache-status"), cache_state)?;

            let elapsed = ctx.timings.request_filter_start.elapsed();
            upstream_response.insert_header(
                HeaderName::from_static("cache-duration"),
                elapsed.as_millis().to_string(),
            )?;
        }

        Ok(())
    }

    /// Each part of the response body, as it's sent to the client (including responses served
    /// from the cache): how long to wait before sending it, when a block paces them
    fn response_body_filter(
        &self,
        session: &mut Session,
        body: &mut Option<bytes::Bytes>,
        _end_of_stream: bool,
        ctx: &mut Self::CTX,
    ) -> pingora::Result<Option<std::time::Duration>> {
        Ok(proksi_plugins::RequestCtx::response_body_delay(
            ctx,
            session,
            body.as_ref(),
        ))
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
            let header = |name: &http::HeaderName| {
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
                || crate::proxy_server::client::Incoming {
                    forwarded_for: header(&crate::proxy_server::client::X_FORWARDED_FOR),
                    proto: header(&crate::proxy_server::client::X_FORWARDED_PROTO),
                    host: header(&crate::proxy_server::client::X_FORWARDED_HOST),
                },
                proto,
                header(&http::header::HOST).or_else(|| session.req_header().uri.host()),
            );
            // `&'static str` names are the cheapest for Pingora: kept as they are for the case of
            // HTTP/1.1 headers (a `HeaderName` is copied first), then parsed once
            upstream_request.insert_header("x-forwarded-for", forwarded.forwarded_for)?;
            upstream_request.insert_header("x-forwarded-proto", forwarded.proto)?;
            if let Some(host) = forwarded.host {
                upstream_request.insert_header("x-forwarded-host", host)?;
            }
            upstream_request.insert_header("x-real-ip", forwarded.real_ip)?;
        }

        // The headers of the upstream chosen, parsed when its location was built
        let added = ctx
            .route_container
            .as_ref()
            .zip(ctx.upstream_index)
            .and_then(|(location, index)| location.upstream_headers.get(index));
        for (name, value) in added.into_iter().flatten() {
            upstream_request
                .insert_header(name.clone(), value.clone())
                .ok();
        }

        if let Some(location) = ctx.route_container.clone() {
            // The location's path rewrite
            if let Some(rewrite) = &location.rewrite {
                rewrite_uri(upstream_request, rewrite);
            }

            location
                .blocks
                .upstream_request(session, upstream_request, ctx)
                .await;
        }
        crate::rules::upstream_request(session, upstream_request, ctx).await;

        // The copy for the location's mirror: the request as sent, without the headers of the
        // upstream it's sent to
        if let Some(location) = &ctx.route_container {
            if location.mirror.is_some() {
                let added = ctx
                    .upstream_index
                    .and_then(|index| location.upstreams.get(index))
                    .and_then(|upstream| upstream.headers.as_ref()?.add.as_ref());
                crate::routing::mirror::start(
                    &mut ctx.mirror,
                    location.mirror.as_ref(),
                    ctx.mirror_rule,
                    upstream_request,
                    added
                        .into_iter()
                        .flatten()
                        .map(|header| header.name.as_ref()),
                );
            }
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
            // Tried again on another upstream (`retry { on = ["502"] }`): idempotent requests
            // whose body was kept, before anything is sent to the client
            let status = upstream_response.status.as_u16();
            let retried = location
                .retry
                .as_ref()
                .is_some_and(|retry| retry.on_status(status, ctx.tries, ctx.retry_rule))
                && session.req_header().method.is_idempotent()
                && !session.as_ref().retry_buffer_truncated();
            if retried {
                let mut e = pingora::Error::explain(
                    HTTPStatus(status),
                    format!("the upstream answered {status}: tried again"),
                );
                e.set_retry(true);
                return Err(e);
            }
            location
                .blocks
                .upstream_response(session, upstream_response, ctx);
        }
        crate::rules::upstream_response(session, upstream_response, ctx);

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
        release_in_flight(ctx);
        if ctx.health_check {
            return;
        }
        let duration = ctx.timings.request_filter_start.elapsed();
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

        // The latest requests, only while someone watches (at most 100 a second)
        if crate::metrics::requests::wanted(&ctx.host) {
            crate::metrics::requests::publish(served(session, ctx, status_code, duration));
        }

        // Nothing to build when access logs aren't written
        let info = tracing::enabled!(tracing::Level::INFO);
        if !info || !crate::services::logger::access_logs_enabled() {
            return;
        }
        let duration_ms = duration.as_millis();

        let http_version = if session.is_http2() {
            "http/2"
        } else {
            "http/1.1"
        };

        let method = session.req_header().method.as_str();
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
            .get(http::header::USER_AGENT)
            .unwrap_or(&empty_header);

        let peer = session.client_addr();
        let forwarded_for = session
            .req_header()
            .headers
            .get(&crate::proxy_server::client::X_FORWARDED_FOR)
            .and_then(|value| value.to_str().ok());
        let client_ip = client_ip(session);

        let (reused, upstream_addr) = match &ctx.upstream_connection {
            Some((reused, peer)) => (if *reused { "true" } else { "false" }, Some(peer)),
            None => ("", None),
        };
        tracing::info!(
            method,
            path,
            query,
            host,
            duration_ms,
            user_agent = user_agent.to_str().unwrap_or(""),
            referer = referer.to_str().unwrap_or(""),
            client_ip,
            // The connection's address: behind trusted proxies, the last of them
            remote_addr = %OrEmpty(peer),
            // As sent, trusted or not (`client_ip` uses it only from trusted proxies)
            x_forwarded_for = forwarded_for.unwrap_or(""),
            status_code,
            http_version,
            reused_connection = reused,
            upstream_addr = %OrEmpty(upstream_addr),
            request_id = ctx.request_id.as_ref().and_then(|id| id.to_str().ok()),
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
        ctx.cache_state = Some("fwd=miss");
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
            ctx.cache_state = Some("expired");
            // Keeps the asset's stale-while-revalidate/stale-if-error windows open,
            // matching the pre-0.9 `ForceExpired` behavior
            return Ok(Some(ForcedFreshness::ForceExpiredServeStale {
                expired_at: None,
            }));
        }

        ctx.cache_state = Some("hit");
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
        let mut e = e;
        // The upstream's name may point elsewhere now (ADR 0006)
        if let Some(route) = &ctx.route_container {
            let load_balancer = route.load_balancer.clone();
            tokio::spawn(async move { load_balancer.update().await });
            // Another upstream may be reached (`retry { on = ["connect"] }`)
            if route
                .retry
                .as_ref()
                .is_some_and(|retry| retry.on_connect(ctx.tries, ctx.retry_rule))
            {
                e.set_retry(true);
            }
        }
        e
    }

    /// Answers a request that couldn't be proxied: `504` when the upstream didn't answer in time,
    /// `502` for its other errors (as Pingora does)
    async fn fail_to_proxy(
        &self,
        session: &mut Session,
        e: &pingora::Error,
        ctx: &mut Self::CTX,
    ) -> FailToProxy {
        let code = error_status(e);
        if code > 0 {
            if let Err(err) = respond_error(session, ctx, code).await {
                tracing::error!("failed to send error response to downstream: {err}");
            }
        }
        FailToProxy {
            error_code: code,
            can_reuse_downstream: false,
        }
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
        ctx.upstream_connection = Some((reused, peer.address().clone()));
        Ok(())
    }
}

/// The status answered for a request that couldn't be proxied (`0`: the client is gone)
fn error_status(e: &pingora::Error) -> u16 {
    use pingora::{ErrorSource, ErrorType};
    match (e.etype(), e.esource()) {
        (HTTPStatus(code), _) => *code,
        (
            ErrorType::ConnectTimedout
            | ErrorType::TLSHandshakeTimedout
            | ErrorType::ReadTimedout
            | ErrorType::WriteTimedout,
            ErrorSource::Upstream,
        ) => 504,
        (_, ErrorSource::Upstream) => 502,
        (ErrorType::ReadTimedout, ErrorSource::Downstream) => 408,
        (
            ErrorType::WriteError | ErrorType::ReadError | ErrorType::ConnectionClosed,
            ErrorSource::Downstream,
        ) => 0,
        (_, ErrorSource::Downstream) => 400,
        (_, ErrorSource::Internal | ErrorSource::Unset) => 500,
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

/// The connection's address, or behind trusted proxies, the client they forwarded
fn client_ip(session: &Session) -> String {
    let peer = session.client_addr();
    let forwarded_for = session
        .req_header()
        .headers
        .get(&crate::proxy_server::client::X_FORWARDED_FOR)
        .and_then(|value| value.to_str().ok());
    match peer.and_then(|addr| addr.as_inet()) {
        Some(addr) => {
            let client = crate::proxy_server::client::client_ip(addr.ip(), forwarded_for);
            if client == addr.ip() {
                addr.to_string()
            } else {
                client.to_string()
            }
        }
        None => peer.map(ToString::to_string).unwrap_or_default(),
    }
}

/// A request for the overview's latest requests
fn served(
    session: &Session,
    ctx: &RouterContext,
    status: u16,
    duration: Duration,
) -> crate::metrics::requests::Served {
    let request = session.req_header();
    crate::metrics::requests::Served {
        time: crate::metrics::requests::Served::now(),
        method: request.method.to_string(),
        host: ctx.host.clone(),
        path: request.uri.path().to_string(),
        status,
        duration_ms: duration.as_secs_f64() * 1000.0,
        client_ip: client_ip(session),
        location: ctx
            .route_container
            .as_ref()
            .map(|location| location.path.clone()),
        upstream: ctx
            .upstream_connection
            .as_ref()
            .map(|(_, peer)| peer.to_string()),
    }
}

/// Shows nothing for `None`: access log fields are there for every request
struct OrEmpty<T>(Option<T>);

impl<T: std::fmt::Display> std::fmt::Display for OrEmpty<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.0 {
            Some(value) => value.fmt(f),
            None => Ok(()),
        }
    }
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

/// A plain HTTP request for a host with TLS, or an unknown one: an ACME challenge, answered by
/// proksi or proxied to a location of its own (e.g. cert-manager's HTTP-01 solver), else a
/// redirect to HTTPS. The host to proxy it to, or `None` once answered.
async fn acme_or_redirect(
    session: &mut Session,
    ctx: &mut RouterContext,
) -> pingora::Result<Option<Arc<stores::host_container::HostContainer>>> {
    let path = session.req_header().uri.path().to_string();
    let req_host = get_host(session).to_string();
    if !path.starts_with(http_proxy::ACME_CHALLENGES) {
        let port =
            stores::lookup::named_host(&ctx.host).and_then(|host| host.https_redirect_port());
        http_proxy::redirect_to_https(session, &req_host, port).await?;
        return Ok(None);
    }
    if http_proxy::serve_acme_challenge(session, &req_host).await? {
        return Ok(None);
    }
    let found = stores::lookup::find_host(&ctx.host, |host| {
        host.location_for(&path)
            .is_some_and(|location| location.path.starts_with(http_proxy::ACME_CHALLENGES))
    });
    if found.is_none() {
        respond_error(session, ctx, 404).await?;
    }
    Ok(found)
}

/// The path sent to the upstream, rewritten (its query kept)
fn rewrite_uri(request: &mut RequestHeader, rewrite: &crate::config::host::CompiledRewrite) {
    let uri = &request.uri;
    let rewritten = rewrite.apply(uri.path(), uri.query());
    if let Ok(uri) = rewritten.parse::<http::Uri>() {
        request.set_uri(uri);
    }
}

/// The upstream chosen no longer counts the request as in progress (least connections)
fn release_in_flight(ctx: &mut RouterContext) {
    if let Some(count) = ctx.in_flight.take() {
        count.fetch_sub(1, std::sync::atomic::Ordering::Relaxed);
    }
}
