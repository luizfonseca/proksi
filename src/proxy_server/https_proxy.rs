use std::{borrow::Cow, collections::HashMap, sync::Arc};

use async_trait::async_trait;

use http::{HeaderValue, Uri};
use pingora::http::{RequestHeader, ResponseHeader};
use pingora::proxy::{ProxyHttp, Session};
use pingora::{upstreams::peer::HttpPeer, ErrorType::HTTPStatus};

use crate::stores::{self, routes::RouteStoreContainer};

use super::{
    middleware::{
        execute_request_plugins, execute_response_plugins, execute_upstream_request_plugins,
        execute_upstream_response_plugins,
    },
    DEFAULT_PEER_OPTIONS,
};

/// Load balancer proxy struct
pub struct Router {
    // pub store: RouteStore,
}

fn process_route(ctx: &RouterContext) -> Arc<RouteStoreContainer> {
    ctx.route_container.clone().unwrap()
}
pub struct RouterContext {
    pub host: String,
    pub route_container: Option<Arc<RouteStoreContainer>>,
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
            route_container: None,
            extensions: HashMap::new(),
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

        // If there's no host matching, returns a 404
        let Some(route_container) = stores::get_route_by_key(host_without_port) else {
            session.respond_error(404).await;
            return Ok(true);
        };

        let arced = Arc::new(route_container);

        // Match request pattern based on the URI
        let uri = get_uri(session);

        match &arced.path_matcher.pattern {
            Some(pattern) if pattern.find(uri.path()).is_none() => {
                session.respond_error(404).await;
                return Ok(true);
            }
            _ => {}
        }

        ctx.route_container = Some(Arc::clone(&arced));

        // Middleware phase: request_filterx
        // We are checking to see if the request has already been handled
        // by the plugins i.e. (ok(true))
        if let Ok(true) = execute_request_plugins(session, ctx, &arced.plugins).await {
            return Ok(true);
        }

        Ok(false)
    }

    /// Define where the proxy should send the request to.
    ///
    /// The returned [HttpPeer] contains the information regarding
    /// where and how this request should forwarded to."]
    async fn upstream_peer(
        &self,
        _session: &mut Session,
        ctx: &mut Self::CTX,
    ) -> pingora::Result<Box<HttpPeer>> {
        // If there's no host matching, returns a 404
        let route_container = process_route(ctx);

        let Some(healthy_upstream) = route_container.load_balancer.select(b"", 4) else {
            return Err(pingora::Error::new(HTTPStatus(503)));
        };

        // https://github.com/cloudflare/pingora/blob/main/docs/user_guide/peer.md?plain=1#L17
        let mut peer = HttpPeer::new(healthy_upstream, false, ctx.host.clone());
        peer.options = DEFAULT_PEER_OPTIONS;
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
        let route_container = process_route(ctx);

        for (name, value) in &route_container.host_header_add {
            upstream_response.insert_header(name, value)?;
        }

        // Remove headers from the upstream response
        for name in &route_container.host_header_remove {
            upstream_response.remove_header(name);
        }

        // Middleware phase: response_filterx
        execute_response_plugins(&route_container, session, ctx).await?;

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
    ) -> pingora::Result<()>
    where
        Self::CTX: Send + Sync,
    {
        // If there's no host matching, returns a 404
        let route_container = process_route(ctx);

        execute_upstream_request_plugins(&route_container, session, upstream_request, ctx)
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
    ) {
        // If there's no host matching, returns a 404
        let route_container = process_route(ctx);

        execute_upstream_response_plugins(&route_container, session, upstream_response, ctx);

        //
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

        let duration_ms = ctx.timings.request_filter_start.elapsed().as_millis();

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
            request_id = ctx.extensions.get("request_id_header"),
            access_log = true
        );
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
