//! The client of a request: the connection's address, or behind trusted proxies, the address
//! they forwarded (`X-Forwarded-For`)

use std::{net::IpAddr, sync::OnceLock};

use ipnet::IpNet;

/// Proxies (addresses or networks) whose `X-Forwarded-For` is trusted
#[derive(Debug, Default, Clone, PartialEq)]
pub struct TrustedProxies(Vec<IpNet>);

static TRUSTED: OnceLock<TrustedProxies> = OnceLock::new();

impl TrustedProxies {
    /// From addresses (`10.0.0.1`) and networks (`10.0.0.0/8`)
    pub fn parse(proxies: &[String]) -> Result<Self, String> {
        proxies
            .iter()
            .map(|proxy| {
                proxy
                    .parse::<IpNet>()
                    .or_else(|_| proxy.parse::<IpAddr>().map(IpNet::from))
                    .map_err(|_| {
                        format!(
                            "trusted_proxies: {proxy:?} is not an address or a network (10.0.0.0/8)"
                        )
                    })
            })
            .collect::<Result<Vec<_>, _>>()
            .map(Self)
    }

    fn trusts(&self, ip: &IpAddr) -> bool {
        self.0.iter().any(|network| network.contains(ip))
    }

    /// The client of a connection from `peer`: `peer` itself, unless it's a trusted proxy.
    /// Then, going from the right of `X-Forwarded-For` (the closest hop), the first address
    /// that isn't a trusted proxy. Addresses a client wrote itself (on the left) are never used
    /// when a trusted proxy appended one after them.
    pub fn client(&self, peer: IpAddr, forwarded_for: Option<&str>) -> IpAddr {
        if !self.trusts(&peer) {
            return peer;
        }
        let mut client = peer;
        for hop in forwarded_for.unwrap_or_default().rsplit(',') {
            let Some(ip) = parse_hop(hop.trim()) else {
                break;
            };
            client = ip;
            if !self.trusts(&ip) {
                break;
            }
        }
        client
    }
}

/// An address of `X-Forwarded-For`, with or without a port (`1.2.3.4:80`, `[2001:db8::1]:80`)
fn parse_hop(hop: &str) -> Option<IpAddr> {
    hop.parse::<IpAddr>()
        .ok()
        .or_else(|| hop.parse::<std::net::SocketAddr>().ok().map(|addr| addr.ip()))
}

/// Sets the trusted proxies (once, when proksi starts)
pub fn init(proxies: TrustedProxies) {
    let _ = TRUSTED.set(proxies);
}

/// The client of a request, see [`TrustedProxies::client`]
pub fn client_ip(peer: IpAddr, forwarded_for: Option<&str>) -> IpAddr {
    match TRUSTED.get() {
        Some(trusted) => trusted.client(peer, forwarded_for),
        None => peer,
    }
}

#[cfg(test)]
mod tests {
    use std::net::IpAddr;

    use super::TrustedProxies;

    fn ip(value: &str) -> IpAddr {
        value.parse().unwrap()
    }

    fn trusted(proxies: &[&str]) -> TrustedProxies {
        TrustedProxies::parse(&proxies.iter().map(ToString::to_string).collect::<Vec<_>>())
            .unwrap()
    }

    #[test]
    fn test_parse() {
        assert!(TrustedProxies::parse(&["10.0.0.0/8".into(), "127.0.0.1".into(), "::1".into()]).is_ok());
        assert_eq!(
            TrustedProxies::parse(&["10.0.0".into()]),
            Err("trusted_proxies: \"10.0.0\" is not an address or a network (10.0.0.0/8)".to_string())
        );
    }

    #[test]
    fn test_untrusted_connections_are_the_client() {
        let proxies = trusted(&["10.0.0.0/8"]);

        // Whatever they send
        assert_eq!(proxies.client(ip("203.0.113.9"), Some("1.2.3.4")), ip("203.0.113.9"));
        assert_eq!(TrustedProxies::default().client(ip("10.0.0.1"), Some("1.2.3.4")), ip("10.0.0.1"));
    }

    #[test]
    fn test_behind_trusted_proxies() {
        let proxies = trusted(&["10.0.0.0/8", "192.168.1.1"]);

        assert_eq!(proxies.client(ip("10.0.0.1"), Some("203.0.113.9")), ip("203.0.113.9"));
        // A chain of trusted proxies
        assert_eq!(
            proxies.client(ip("10.0.0.1"), Some("203.0.113.9, 192.168.1.1, 10.1.1.1")),
            ip("203.0.113.9")
        );
        // An address the client wrote itself, on the left, isn't used
        assert_eq!(
            proxies.client(ip("10.0.0.1"), Some("1.2.3.4, 203.0.113.9")),
            ip("203.0.113.9")
        );
        // Several headers are joined with commas by the time they're read; IPv6, ports
        assert_eq!(proxies.client(ip("10.0.0.1"), Some(" 2001:db8::1 ")), ip("2001:db8::1"));
        assert_eq!(proxies.client(ip("10.0.0.1"), Some("203.0.113.9:4711")), ip("203.0.113.9"));
        assert_eq!(proxies.client(ip("10.0.0.1"), Some("[2001:db8::1]:4711")), ip("2001:db8::1"));
    }

    #[test]
    fn test_without_a_usable_header() {
        let proxies = trusted(&["10.0.0.0/8"]);

        // The proxy itself
        assert_eq!(proxies.client(ip("10.0.0.1"), None), ip("10.0.0.1"));
        assert_eq!(proxies.client(ip("10.0.0.1"), Some("")), ip("10.0.0.1"));
        // Only trusted proxies: the furthest one
        assert_eq!(proxies.client(ip("10.0.0.1"), Some("10.0.0.2, 10.0.0.3")), ip("10.0.0.2"));
        // Garbage stops the walk: the last address that was valid
        assert_eq!(proxies.client(ip("10.0.0.1"), Some("unknown, 10.0.0.2")), ip("10.0.0.2"));
    }
}
