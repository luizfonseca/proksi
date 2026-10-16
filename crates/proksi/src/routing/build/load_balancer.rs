//! Load balancers between upstreams, shared by locations and `tcp` blocks

use std::{
    collections::BTreeSet,
    future::Future,
    net::ToSocketAddrs,
    task::{Context, Waker},
};

use http::Extensions;
use pingora::{
    lb::{discovery::Static, selection::RoundRobin, Backend, Backends, LoadBalancer},
    protocols::l4::socket::SocketAddr,
};

use crate::config::RouteUpstream;

/// Round-robin between the upstreams, in proportion to their weight (1 by default).
/// Hostnames are resolved here, as with [`LoadBalancer::try_from_iter`].
pub fn load_balancer(upstreams: &[RouteUpstream]) -> std::io::Result<LoadBalancer<RoundRobin>> {
    let mut backends = BTreeSet::new();
    for upstream in upstreams {
        let weight = upstream
            .weight
            .and_then(|weight| usize::try_from(weight).ok())
            .unwrap_or(1);
        let addrs = format!("{}:{}", upstream.ip, upstream.port).to_socket_addrs()?;
        backends.extend(addrs.map(|addr| Backend {
            addr: SocketAddr::Inet(addr),
            weight,
            ext: Extensions::new(),
        }));
    }

    let load_balancer = LoadBalancer::from_backends(Backends::new(Static::new(backends)));
    // Loads the backends: static discovery completes on the first poll
    {
        let update = std::pin::pin!(load_balancer.update());
        let _ = update.poll(&mut Context::from_waker(Waker::noop()));
    }
    Ok(load_balancer)
}
