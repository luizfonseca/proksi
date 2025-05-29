use std::{collections::BTreeMap, time::Duration};

use pingora::{
    protocols::{TcpKeepalive, ALPN},
    upstreams::peer::PeerOptions,
};

pub mod cert_store;
pub mod http_proxy;
pub mod https_proxy;
pub mod middleware;

/// Default peer options to be used on every upstream connection
pub fn default_peer_opts() -> PeerOptions {
    let mut po = PeerOptions::new();

    po.tcp_fast_open = true;
    po.verify_hostname = true;
    po.read_timeout = Some(Duration::from_secs(360));
    po.connection_timeout = Some(Duration::from_secs(10));
    po.tcp_recv_buf = Some(1024 * 8);
    po.tcp_keepalive = Some(TcpKeepalive {
        count: 10,
        idle: Duration::from_secs(60),
        interval: Duration::from_secs(30),
        #[cfg(target_os = "linux")]
        user_timeout: Duration::from_secs(0),
    });
    po.total_connection_timeout = Some(Duration::from_secs(20));
    po.idle_timeout = Some(Duration::from_secs(360));
    po.write_timeout = Some(Duration::from_secs(60));
    po.verify_cert = false;
    po.alternative_cn = None;
    po.alpn = ALPN::H2H1;
    po.ca = None;
    po.h2_ping_interval = Some(Duration::from_secs(60));
    po.max_h2_streams = 2;
    po.extra_proxy_headers = BTreeMap::new();
    po.curves = None;
    po.second_keyshare = true; // default true and noop when not using PQ curves
    po.tracer = None;
    po.custom_l4 = None;
    po
}
