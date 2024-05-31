use std::{collections::BTreeMap, sync::Arc, time::Duration};

use async_trait::async_trait;
use pingora::{
    protocols::ALPN,
    upstreams::peer::{HttpPeer, PeerOptions, TcpKeepalive},
    ErrorType::HTTPStatus,
};
use pingora_http::ResponseHeader;
use pingora_load_balancing::{selection::RoundRobin, LoadBalancer};
use pingora_proxy::{ProxyHttp, Session};
use tracing::info;

use crate::stores::routes::RouteStore;

/// Default peer options to be used on every upstream connection
pub const DEFAULT_PEER_OPTIONS: PeerOptions = PeerOptions {
    verify_hostname: true,
    read_timeout: Some(Duration::from_secs(30)),
    connection_timeout: Some(Duration::from_secs(30)),
    tcp_recv_buf: None,
    tcp_keepalive: Some(TcpKeepalive {
        count: 5,
        interval: Duration::from_secs(15),
        idle: Duration::from_secs(60),
    }),
    bind_to: None,
    total_connection_timeout: None,
    idle_timeout: None,
    write_timeout: None,
    verify_cert: false,
    alternative_cn: None,
    alpn: ALPN::H1,
    ca: None,
    no_header_eos: false,
    h2_ping_interval: None,
    max_h2_streams: 1,
    extra_proxy_headers: BTreeMap::new(),
    curves: None,
    second_keyshare: true, // default true and noop when not using PQ curves
    tracer: None,
};

type ArcedLB = Arc<LoadBalancer<RoundRobin>>;
/// Load balancer proxy struct
pub struct Router {
    pub store: RouteStore,
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
        let host_without_port = req_host.split(':').collect::<Vec<_>>()[0];

        // If there's no host matching, returns a 404
        let route_container = self.store.get(host_without_port);
        if route_container.is_none() {
            session.respond_error(404).await;
            return Ok(true);
        }

        // Match request pattern based on the URI
        let uri = session.req_header().uri.clone();
        let route_container = route_container.unwrap();

        match &route_container.path_matcher.pattern {
            Some(pattern) if pattern.find(uri.path()).is_none() => {
                session.respond_error(404).await;
                return Ok(true);
            }
            _ => {}
        }

        ctx.host = host_without_port.to_string();
        ctx.current_lb = Some(route_container.load_balancer.clone());
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
        let upstream = ctx.current_lb.clone();

        // No upstream found (should never happen, but just in case)
        if upstream.is_none() {
            return Err(pingora::Error::new(HTTPStatus(404)));
        }

        // No healthy upstream found
        let healthy_upstream = upstream.unwrap().select(b"", 32);
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
        _session: &mut Session,
        _upstream_response: &mut ResponseHeader,
        _ctx: &mut Self::CTX,
    ) -> pingora::Result<()> {
        // Add custom headers to the response

        Ok(())
    }
}

/// Retrieves the host from the request headers based on
/// whether the request is HTTP/1.1 or HTTP/2
fn get_host(session: &Session) -> &str {
    if let Some(host) = session.get_header(http::header::HOST) {
        return host.to_str().unwrap_or("");
    }

    if let Some(host) = session.req_header().uri.host() {
        return host;
    }

    ""
}
