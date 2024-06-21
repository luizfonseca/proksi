use std::{collections::BTreeMap, time::Duration};

use pingora::{
    protocols::ALPN,
    upstreams::peer::{PeerOptions, TcpKeepalive},
};

pub mod cert_store;
pub mod http_proxy;
pub mod https_proxy;
pub mod middleware;

/// Default peer options to be used on every upstream connection
const DEFAULT_PEER_OPTIONS: PeerOptions = PeerOptions {
    verify_hostname: true,
    read_timeout: Some(Duration::from_secs(360)),
    connection_timeout: Some(Duration::from_secs(10)),
    tcp_recv_buf: Some(1024 * 8),
    tcp_keepalive: Some(TcpKeepalive {
        count: 10,
        idle: Duration::from_secs(60),
        interval: Duration::from_secs(30),
    }),
    bind_to: None,
    total_connection_timeout: Some(Duration::from_secs(20)),
    idle_timeout: Some(Duration::from_secs(360)),
    write_timeout: Some(Duration::from_secs(60)),
    verify_cert: false,
    alternative_cn: None,
    alpn: ALPN::H2H1,
    ca: None,
    no_header_eos: true,
    h2_ping_interval: Some(Duration::from_secs(60)),
    max_h2_streams: 2,
    extra_proxy_headers: BTreeMap::new(),
    curves: None,
    second_keyshare: true, // default true and noop when not using PQ curves
    tracer: None,
};
