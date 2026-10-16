//! TCP proxying (`tcp` blocks, see ADR 0003): connections accepted on an address are
//! forwarded to the upstreams of its block

use std::{
    net::SocketAddr,
    sync::{Arc, OnceLock},
};

use pingora::lb::{selection::RoundRobin, LoadBalancer};

use crate::{config::tcp::TcpTls, stores::host_container::binds};

pub mod alpn;
pub mod proxy;
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
            TcpTls::None | TcpTls::Passthrough => true,
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
        assert_eq!(
            chosen(&targets, "127.0.0.1:6000", None).as_deref(),
            Some("raw")
        );
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
}
