use std::{collections::BTreeMap, time::Duration};

use pingora::{
    protocols::{TcpKeepalive, ALPN},
    upstreams::peer::PeerOptions,
};

use crate::config::host::UpstreamTimeouts;

pub mod cert_store;
pub mod client;
pub mod connections;
pub mod http_proxy;
pub mod https_proxy;
pub mod listener;
pub mod respond;
pub mod services;
pub mod timeouts;

/// Peer options of every upstream connection, with `timeouts` (the location's, then the server's)
/// and the default ones where they aren't set
pub fn default_peer_opts(timeouts: UpstreamTimeouts) -> PeerOptions {
    let secs = |secs: Option<u64>, default: u64| Duration::from_secs(secs.unwrap_or(default));
    let connect = secs(timeouts.connect_secs, 5);
    let mut po = PeerOptions::new();

    po.tcp_fast_open = true;
    po.verify_hostname = true;
    po.read_timeout = Some(secs(timeouts.read_secs, 360));
    po.connection_timeout = Some(connect);
    // The TLS handshake of HTTPS upstreams, after connecting
    po.total_connection_timeout = Some(connect + secs(timeouts.tls_secs, 10));
    po.tcp_recv_buf = Some(1024 * 8);
    po.tcp_keepalive = Some(TcpKeepalive {
        count: 10,
        idle: Duration::from_secs(60),
        interval: Duration::from_secs(30),
        #[cfg(target_os = "linux")]
        user_timeout: Duration::from_secs(0),
    });
    po.idle_timeout = Some(secs(timeouts.idle_secs, 360));
    po.write_timeout = Some(secs(timeouts.write_secs, 60));
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
