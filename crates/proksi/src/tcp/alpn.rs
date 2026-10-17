//! Choosing the protocol (ALPN) of a TLS connection, so a host can be HTTPS and TCP on one
//! address: the protocols of its `tcp` blocks come first, then HTTP/2 and HTTP/1.1

use std::collections::HashMap;

use pingora::{
    listeners::tls::TlsSettings,
    tls::ssl::{AlpnError, NameType},
};

use crate::config::tcp::TcpTls;

use super::TcpTarget;

/// The protocols of the `tcp` blocks, by host
#[derive(Debug, Default)]
pub struct AlpnProtocols {
    by_host: HashMap<String, Vec<String>>,
}

impl AlpnProtocols {
    pub fn new<'a>(targets: impl IntoIterator<Item = &'a TcpTarget>) -> Self {
        let mut by_host: HashMap<String, Vec<String>> = HashMap::new();
        for target in targets {
            if let (TcpTls::Terminate, Some(host)) = (target.tls, &target.host) {
                if !target.alpn.is_empty() {
                    by_host
                        .entry(host.to_lowercase())
                        .or_default()
                        .extend(target.alpn.iter().cloned());
                }
            }
        }
        Self { by_host }
    }

    pub fn is_empty(&self) -> bool {
        self.by_host.is_empty()
    }

    /// The protocol for a client asking for `server_name` and offering `offered` (ALPN wire
    /// format), or none. With `http`, HTTP/2 and HTTP/1.1 when no `tcp` block's protocol is.
    pub fn select<'a>(
        &self,
        server_name: Option<&str>,
        offered: &'a [u8],
        http: bool,
    ) -> Option<&'a [u8]> {
        let offered = parse(offered)?;
        let find = |protocol: &[u8]| offered.iter().copied().find(|p| *p == protocol);

        let tcp = server_name.and_then(|name| self.by_host.get(&name.to_lowercase()));
        // In the client's order of preference
        if let Some(protocols) = tcp {
            let chosen = offered
                .iter()
                .copied()
                .find(|p| protocols.iter().any(|protocol| protocol.as_bytes() == *p));
            if chosen.is_some() {
                return chosen;
            }
        }
        if http {
            return find(b"h2").or_else(|| find(b"http/1.1"));
        }
        None
    }
}

/// Selects the protocols of the `tcp` blocks on a TLS listener (and with `http`, HTTP/2 and
/// HTTP/1.1). Without any, the listener keeps its own selection.
pub fn select_protocols(settings: &mut TlsSettings, protocols: AlpnProtocols, http: bool) {
    if protocols.is_empty() {
        return;
    }
    settings.set_alpn_select_callback(move |ssl, offered| {
        protocols
            .select(ssl.servername(NameType::HOST_NAME), offered, http)
            .ok_or(AlpnError::NOACK)
    });
}

/// The protocols of an ALPN list (each prefixed by its length), or none if it is malformed
fn parse(mut wire: &[u8]) -> Option<Vec<&[u8]>> {
    let mut protocols = vec![];
    while let Some((&len, rest)) = wire.split_first() {
        let (protocol, rest) = rest.split_at_checked(usize::from(len))?;
        protocols.push(protocol);
        wire = rest;
    }
    (!protocols.is_empty()).then_some(protocols)
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use pingora::lb::LoadBalancer;

    use crate::config::tcp::TcpTls;

    use super::{AlpnProtocols, TcpTarget};

    fn target(host: &str, tls: TcpTls, alpn: &[&str]) -> TcpTarget {
        TcpTarget {
            label: "tcp".to_string(),
            host: Some(host.to_string()),
            listen: "0.0.0.0:443".parse().unwrap(),
            tls,
            alpn: alpn.iter().map(ToString::to_string).collect(),
            connect_timeout: std::time::Duration::from_secs(5),
            load_balancer: Arc::new(LoadBalancer::try_from_iter(["127.0.0.1:1"]).unwrap()),
            metrics: None,
        }
    }

    fn protocols() -> AlpnProtocols {
        AlpnProtocols::new(&[
            target("a.test", TcpTls::Terminate, &["game/1", "game/2"]),
            target("b.test", TcpTls::Terminate, &[]),
        ])
    }

    /// ALPN wire format: each protocol prefixed by its length
    fn wire(protocols: &[&str]) -> Vec<u8> {
        protocols
            .iter()
            .flat_map(|p| std::iter::once(u8::try_from(p.len()).unwrap()).chain(p.bytes()))
            .collect()
    }

    fn selected(server_name: Option<&str>, offered: &[&str], http: bool) -> Option<String> {
        let offered = wire(offered);
        protocols()
            .select(server_name, &offered, http)
            .map(|p| String::from_utf8(p.to_vec()).unwrap())
    }

    #[test]
    fn test_tcp_protocols_of_the_host_come_first() {
        assert_eq!(
            selected(Some("a.test"), &["h2", "game/2"], true).as_deref(),
            Some("game/2")
        );
        assert_eq!(
            selected(Some("A.Test"), &["game/1"], false).as_deref(),
            Some("game/1")
        );
    }

    #[test]
    fn test_http_otherwise() {
        assert_eq!(
            selected(Some("a.test"), &["http/1.1", "h2"], true).as_deref(),
            Some("h2")
        );
        assert_eq!(
            selected(Some("a.test"), &["http/1.1"], true).as_deref(),
            Some("http/1.1")
        );
        // Another host's protocols aren't selected
        assert_eq!(
            selected(Some("b.test"), &["game/1", "h2"], true).as_deref(),
            Some("h2")
        );
        assert_eq!(selected(None, &["game/1"], true), None);
        // Without HTTP (on a TCP address), nothing
        assert_eq!(selected(Some("b.test"), &["h2"], false), None);
    }

    #[test]
    fn test_invalid_offers() {
        let protocols = protocols();
        assert_eq!(protocols.select(Some("a.test"), &[], true), None);
        // A length longer than what is left
        assert_eq!(protocols.select(Some("a.test"), &[9, b'g'], true), None);
    }

    #[test]
    fn test_without_protocols() {
        assert!(!protocols().is_empty());
        assert!(AlpnProtocols::new(&[target("b.test", TcpTls::Terminate, &[])]).is_empty());
    }
}
