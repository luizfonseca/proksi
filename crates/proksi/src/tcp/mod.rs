//! TCP proxying (`tcp` blocks, see ADR 0003): connections accepted on an address are
//! forwarded to the upstreams of its block

use std::{
    net::SocketAddr,
    sync::{Arc, OnceLock},
};

use pingora::lb::{selection::RoundRobin, LoadBalancer};

use crate::{config::tcp::TcpTls, metrics::TcpMetrics, stores::host_container::binds};

pub mod alpn;
pub mod client_hello;
pub mod proxy;
pub mod router;
pub mod services;

/// A `tcp` block, ready to accept connections
pub struct TcpTarget {
    /// How the block is named in logs (`tcp "db" in host "a.com"`)
    pub label: String,
    pub host: Option<String>,
    pub listen: SocketAddr,
    pub tls: TcpTls,
    /// Protocols (ALPN) the block is chosen by, if any
    pub alpn: Vec<String>,
    pub load_balancer: Arc<LoadBalancer<RoundRobin>>,
    pub metrics: TcpMetrics,
}

/// The load balancers of the `tcp` blocks (set once, at startup)
static LOAD_BALANCERS: OnceLock<Vec<(String, Arc<LoadBalancer<RoundRobin>>)>> = OnceLock::new();

fn register_load_balancers(targets: &[TcpTarget]) {
    let load_balancers = targets
        .iter()
        .map(|target| (target.label.clone(), target.load_balancer.clone()))
        .collect();
    let _ = LOAD_BALANCERS.set(load_balancers);
}

/// The load balancers of the `tcp` blocks, by block, for health checks
pub fn load_balancers() -> &'static [(String, Arc<LoadBalancer<RoundRobin>>)] {
    LOAD_BALANCERS.get().map_or(&[], Vec::as_slice)
}

/// The TLS passthrough block for a connection accepted on `local`, asking for `server_name`
/// (read from its `ClientHello`). On an address of its own, a block takes every connection;
/// shared with other blocks or HTTPS hosts (`shared_with_https`), only those asking for its
/// host.
pub fn passthrough_for<'a>(
    targets: &'a [TcpTarget],
    local: &SocketAddr,
    server_name: Option<&str>,
    shared_with_https: bool,
) -> Option<&'a TcpTarget> {
    let on_address = targets
        .iter()
        .filter(|target| binds(&target.listen, local))
        .collect::<Vec<_>>();
    let alone = on_address.len() == 1 && !shared_with_https;
    on_address.into_iter().find(|target| {
        target.tls == TcpTls::Passthrough
            && (alone
                || target
                    .host
                    .as_deref()
                    .zip(server_name)
                    .is_some_and(|(host, name)| host.eq_ignore_ascii_case(name)))
    })
}

/// The blocks listening on `address`
pub fn on_address<'a>(
    targets: &'a [TcpTarget],
    address: &'a str,
) -> impl Iterator<Item = &'a TcpTarget> {
    targets
        .iter()
        .filter(move |target| target.listen.to_string() == address)
}

/// The block for a connection accepted on `local`. With TLS terminated, the block of the host
/// the client asked for (`sni`), so a host's certificate never reaches another host's block,
/// and if the block has protocols, the one negotiated (`alpn`) must be one of them.
pub fn choose<'a>(
    targets: &'a [TcpTarget],
    local: &SocketAddr,
    sni: Option<&str>,
    alpn: Option<&[u8]>,
) -> Option<&'a TcpTarget> {
    targets
        .iter()
        .filter(|target| binds(&target.listen, local))
        .find(|target| match target.tls {
            TcpTls::Terminate => {
                let host = target
                    .host
                    .as_deref()
                    .zip(sni)
                    .is_some_and(|(host, sni)| host.eq_ignore_ascii_case(sni));
                let protocol = target.alpn.is_empty()
                    || alpn.is_some_and(|alpn| target.alpn.iter().any(|p| p.as_bytes() == alpn));
                host && protocol
            }
            // Only one block per address (checked when the configuration loads)
            TcpTls::None => true,
            // Chosen before any handshake, by `tcp::router` (`passthrough_for`)
            TcpTls::Passthrough => false,
        })
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use pingora::lb::LoadBalancer;

    use crate::config::tcp::TcpTls;

    use super::{choose, TcpTarget};

    fn target(label: &str, host: Option<&str>, listen: &str, tls: TcpTls) -> TcpTarget {
        TcpTarget {
            label: label.to_string(),
            host: host.map(ToString::to_string),
            listen: listen.parse().unwrap(),
            tls,
            alpn: vec![],
            load_balancer: Arc::new(LoadBalancer::try_from_iter(["127.0.0.1:1"]).unwrap()),
            metrics: crate::metrics::TcpMetrics::new(label),
        }
    }

    fn chosen(targets: &[TcpTarget], local: &str, sni: Option<&str>) -> Option<String> {
        chosen_with(targets, local, sni, None)
    }

    fn chosen_with(
        targets: &[TcpTarget],
        local: &str,
        sni: Option<&str>,
        alpn: Option<&str>,
    ) -> Option<String> {
        choose(
            targets,
            &local.parse().unwrap(),
            sni,
            alpn.map(str::as_bytes),
        )
        .map(|target| target.label.clone())
    }

    #[test]
    fn test_choose_by_address() {
        let targets = [
            target("game", None, "0.0.0.0:7777", TcpTls::None),
            target("raw", Some("a.test"), "127.0.0.1:6000", TcpTls::Passthrough),
        ];

        assert_eq!(
            chosen(&targets, "10.0.0.1:7777", None).as_deref(),
            Some("game")
        );
        // TLS passthrough is chosen before any handshake (`passthrough_for`), never here
        assert_eq!(chosen(&targets, "127.0.0.1:6000", None), None);
        assert_eq!(chosen(&targets, "10.0.0.1:6000", None), None);
        assert_eq!(chosen(&targets, "10.0.0.1:8000", None), None);
    }

    #[test]
    fn test_choose_by_server_name_with_tls() {
        let targets = [
            target("a", Some("a.test"), "0.0.0.0:5433", TcpTls::Terminate),
            target("b", Some("b.test"), "0.0.0.0:5433", TcpTls::Terminate),
        ];

        assert_eq!(
            chosen(&targets, "10.0.0.1:5433", Some("a.test")).as_deref(),
            Some("a")
        );
        assert_eq!(
            chosen(&targets, "10.0.0.1:5433", Some("B.Test")).as_deref(),
            Some("b")
        );
        // Another name (or none) never reaches a host's block, even alone on its address
        assert_eq!(chosen(&targets, "10.0.0.1:5433", Some("c.test")), None);
        assert_eq!(chosen(&targets[..1], "10.0.0.1:5433", Some("c.test")), None);
        assert_eq!(chosen(&targets[..1], "10.0.0.1:5433", None), None);
    }

    #[test]
    fn test_choose_by_protocol() {
        let mut game = target("game", Some("a.test"), "0.0.0.0:443", TcpTls::Terminate);
        game.alpn = vec!["game/1".to_string()];
        let targets = [game];

        assert_eq!(
            chosen_with(&targets, "10.0.0.1:443", Some("a.test"), Some("game/1")).as_deref(),
            Some("game")
        );
        // Other protocols (or none) are the host's locations
        assert_eq!(
            chosen_with(&targets, "10.0.0.1:443", Some("a.test"), Some("h2")),
            None
        );
        assert_eq!(
            chosen_with(&targets, "10.0.0.1:443", Some("a.test"), None),
            None
        );
        assert_eq!(
            chosen_with(&targets, "10.0.0.1:443", Some("b.test"), Some("game/1")),
            None
        );
    }

    #[test]
    fn test_passthrough_by_server_name() {
        let targets = [
            target("a", Some("a.test"), "0.0.0.0:443", TcpTls::Passthrough),
            target("b", Some("b.test"), "0.0.0.0:443", TcpTls::Passthrough),
            target("c", Some("c.test"), "0.0.0.0:8443", TcpTls::Passthrough),
        ];
        let chosen = |local: &str, name: Option<&str>, https: bool| {
            super::passthrough_for(&targets, &local.parse().unwrap(), name, https)
                .map(|target| target.label.clone())
        };

        assert_eq!(chosen("10.0.0.1:443", Some("a.test"), true).as_deref(), Some("a"));
        assert_eq!(chosen("10.0.0.1:443", Some("b.test"), false).as_deref(), Some("b"));
        assert_eq!(chosen("10.0.0.1:443", Some("other.test"), true), None);
        assert_eq!(chosen("10.0.0.1:443", None, false), None);
        // Alone on its address: every connection
        assert_eq!(chosen("10.0.0.1:8443", Some("other.test"), false).as_deref(), Some("c"));
        assert_eq!(chosen("10.0.0.1:8443", None, false).as_deref(), Some("c"));
        // Unless the address is shared with HTTPS hosts
        assert_eq!(chosen("10.0.0.1:8443", Some("other.test"), true), None);
        assert_eq!(chosen("10.0.0.1:8443", Some("C.Test"), true).as_deref(), Some("c"));
    }
}
