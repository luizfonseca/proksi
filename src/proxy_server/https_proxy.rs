use std::{collections::HashMap, sync::Arc};

use async_trait::async_trait;
use pingora::{upstreams::peer::HttpPeer, ErrorType::HTTPStatus};
use pingora_load_balancing::{selection::RoundRobin, LoadBalancer};
use pingora_proxy::{ProxyHttp, Session};
use tracing::info;

type ArcedLB = Arc<LoadBalancer<RoundRobin>>;
/// Load balancer proxy struct
pub struct Router {
    routes: HashMap<String, Arc<LoadBalancer<RoundRobin>>>,
}

impl Router {
    pub fn new() -> Self {
        Router {
            routes: HashMap::new(),
        }
    }

    pub fn add_route(&mut self, route: String, upstream: ArcedLB) {
        self.routes.insert(route, upstream);
    }
}

pub struct RouterContext {
    pub host: String,
    pub current_lb: Option<ArcedLB>,
}

#[async_trait]
impl ProxyHttp for Router {
    /// The per request object to share state across the different filters
    type CTX = RouterContext;

    /// Define how the `ctx` should be created.
    fn new_ctx(&self) -> Self::CTX {
        RouterContext {
            host: String::new(),
            current_lb: None,
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
        let host_without_port = req_host.split(':').collect::<Vec<&str>>()[0].to_string();

        // If there's no host matching, returns a 404
        let upstream_lb = self.routes.get(&host_without_port);
        if upstream_lb.is_none() {
            return Err(pingora::Error::new(HTTPStatus(404)));
        }

        ctx.host = host_without_port;
        ctx.current_lb = Some(upstream_lb.unwrap().clone());
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
        let upstream = ctx.current_lb.as_ref();

        // No upstream found (should never happen, but just in case)
        if upstream.is_none() {
            return Err(pingora::Error::new(HTTPStatus(404)));
        }

        // No healthy upstream found
        let healthy_upstream = upstream.unwrap().select(b"", 256);
        if healthy_upstream.is_none() {
            return Err(pingora::Error::new(HTTPStatus(503)));
        }

        info!(host = ctx.host, "Upstream selected");

        // https://github.com/cloudflare/pingora/blob/main/docs/user_guide/peer.md?plain=1#L17
        let peer = HttpPeer::new(healthy_upstream.unwrap(), false, ctx.host.clone());
        Ok(Box::new(peer))
    }
}

/// Retrieves the host from the request headers based on whether
/// the request is HTTP/1.1 or HTTP/2
fn get_host(session: &mut Session) -> String {
    if let Some(host) = session.get_header(http::header::HOST) {
        if let Ok(host_str) = host.to_str() {
            return host_str.to_string();
        }
    }

    if let Some(host) = session.req_header().uri.host() {
        return host.to_string();
    }

    "".to_string()
}
