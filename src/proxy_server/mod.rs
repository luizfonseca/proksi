use std::{collections::BTreeMap, time::Duration};

use pingora::{protocols::ALPN, upstreams::peer::PeerOptions};

pub mod cert_store;
pub mod http_proxy;
pub mod https_proxy;
pub mod middleware;

/// Default peer options to be used on every upstream connection
const DEFAULT_PEER_OPTIONS: PeerOptions = PeerOptions {
    verify_hostname: true,
    read_timeout: None,
    connection_timeout: None,
    tcp_recv_buf: None,
    tcp_keepalive: None,
    bind_to: None,
    total_connection_timeout: None,
    idle_timeout: Some(Duration::from_secs(3600)),
    write_timeout: None,
    verify_cert: false,
    alternative_cn: None,
    alpn: ALPN::H2H1,
    ca: None,
    no_header_eos: false,
    h2_ping_interval: None,
    max_h2_streams: 1,
    extra_proxy_headers: BTreeMap::new(),
    curves: None,
    second_keyshare: true, // default true and noop when not using PQ curves
    tracer: None,
};
