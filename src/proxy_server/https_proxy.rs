use std::{borrow::Cow, collections::HashMap, sync::Arc};

use async_trait::async_trait;

use http::Uri;
use pingora::{upstreams::peer::HttpPeer, ErrorType::HTTPStatus};
use pingora_http::ResponseHeader;
use pingora_proxy::{ProxyHttp, Session};
use tracing::info;

use crate::stores::routes::{RouteStore, RouteStoreContainer};

use super::{
    middleware::{execute_request_plugins, execute_response_plugins},
    DEFAULT_PEER_OPTIONS,
};

/// Load balancer proxy struct
pub struct Router {
    pub store: RouteStore,
}

pub struct RouterContext {
    pub host: String,
    pub route_container: Option<Arc<RouteStoreContainer>>,
    pub extensions: HashMap<Cow<'static, str>, String>,
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
        ctx.host = host_without_port.to_string();

        // If there's no host matching, returns a 404
        let route_container = self.store.get(host_without_port);
        if route_container.is_none() {
            session.respond_error(404).await;
            return Ok(true);
        }

        // Match request pattern based on the URI
        let uri = get_uri(session);
        let route_container = route_container.unwrap();

        match &route_container.path_matcher.pattern {
            Some(pattern) if pattern.find(uri.path()).is_none() => {
                session.respond_error(404).await;
                return Ok(true);
            }
            _ => {}
        }

        ctx.route_container = Some(route_container.value().clone());

        // Middleware phase: request_filterx
        // We are checking to see if the request has already been handled
        // by the plugins i.e. (ok(true))
        match execute_request_plugins(session, ctx, &route_container.plugins).await {
            Ok(true) => return Ok(true),
            _ => {}
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
        let upstream = ctx.route_container.as_ref();

        // No upstream found (should never happen, but just in case)
        if upstream.is_none() {
            return Err(pingora::Error::new(HTTPStatus(404)));
        }

        // No healthy upstream found
        let upstream = upstream.unwrap();
        let healthy_upstream = upstream.load_balancer.select(b"", 32);
        if healthy_upstream.is_none() {
            info!("No healthy upstream found");
            return Err(pingora::Error::new(HTTPStatus(503)));
        }

        // https://github.com/cloudflare/pingora/blob/main/docs/user_guide/peer.md?plain=1#L17
        let mut peer = HttpPeer::new(healthy_upstream.unwrap(), false, ctx.host.clone());
        peer.options = DEFAULT_PEER_OPTIONS;
        Ok(Box::new(peer))
    }

    async fn response_filter(
        &self,
        session: &mut Session,
        upstream_response: &mut ResponseHeader,
        ctx: &mut Self::CTX,
    ) -> pingora::Result<()> {
        let container = ctx.route_container.as_ref().unwrap();

        for (name, value) in &container.host_header_add {
            upstream_response.insert_header(name, value)?;
        }

        // Remove headers from the upstream response
        for name in &container.host_header_remove {
            upstream_response.remove_header(name);
        }

        // Middleware phase: response_filterx
        execute_response_plugins(session, ctx, &container.plugins).await?;

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
