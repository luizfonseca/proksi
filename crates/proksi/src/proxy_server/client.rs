//! The client of a request: the connection's address, or behind trusted proxies, the address
//! they forwarded (`X-Forwarded-For`)

use std::{net::IpAddr, sync::OnceLock};

use http::{HeaderName, HeaderValue};

use crate::ip_set::{network, IpSet};

/// Proxies (addresses or networks) whose `X-Forwarded-For` is trusted
#[derive(Debug, Default, Clone, PartialEq)]
pub struct TrustedProxies(IpSet);

static TRUSTED: OnceLock<TrustedProxies> = OnceLock::new();

/// Headers are read by these, so their names aren't parsed on each request (built at compile
/// time, they don't allocate). Headers are inserted by `&'static str` instead, as Pingora keeps
/// those as they are.
pub const X_FORWARDED_FOR: HeaderName = HeaderName::from_static("x-forwarded-for");
pub const X_FORWARDED_PROTO: HeaderName = HeaderName::from_static("x-forwarded-proto");
pub const X_FORWARDED_HOST: HeaderName = HeaderName::from_static("x-forwarded-host");

impl TrustedProxies {
    /// From addresses (`10.0.0.1`) and networks (`10.0.0.0/8`)
    pub fn parse(proxies: &[String]) -> Result<Self, String> {
        proxies
            .iter()
            .map(|proxy| {
                network(proxy).ok_or_else(|| {
                    format!(
                        "trusted_proxies: {proxy:?} is not an address or a network (10.0.0.0/8)"
                    )
                })
            })
            .collect::<Result<Vec<_>, _>>()
            .map(|networks| Self(IpSet::new(networks)))
    }

    fn trusts(&self, ip: &IpAddr) -> bool {
        self.0.contains(*ip)
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
    hop.parse::<IpAddr>().ok().or_else(|| {
        hop.parse::<std::net::SocketAddr>()
            .ok()
            .map(|addr| addr.ip())
    })
}

/// The `X-Forwarded-*` headers of a request, as received
#[derive(Debug, Default, Clone, Copy)]
pub struct Incoming<'a> {
    pub forwarded_for: Option<&'a str>,
    pub proto: Option<&'a str>,
    pub host: Option<&'a str>,
}

/// The headers sent to upstreams about the client
#[derive(Debug, PartialEq)]
#[allow(clippy::struct_field_names)] // named after the headers
pub struct Forwarded {
    pub forwarded_for: HeaderValue,
    pub proto: HeaderValue,
    pub host: Option<HeaderValue>,
    pub real_ip: HeaderValue,
}

impl TrustedProxies {
    /// The headers for a request from `peer`, received over `proto` (`http`, `https`) for
    /// `host`. What a trusted proxy sent (`incoming`) is kept, and `peer` appended to
    /// `X-Forwarded-For`; what anyone else sent is replaced, so clients can't forge them (and
    /// isn't read).
    pub fn forwarded<'a>(
        &self,
        peer: IpAddr,
        incoming: impl FnOnce() -> Incoming<'a>,
        proto: &'static str,
        host: Option<&str>,
    ) -> Forwarded {
        let host = host.and_then(|host| HeaderValue::from_str(host).ok());
        if !self.trusts(&peer) {
            let peer = ip_value(peer);
            return Forwarded {
                forwarded_for: peer.clone(),
                proto: HeaderValue::from_static(proto),
                host,
                real_ip: peer,
            };
        }

        let incoming = incoming();
        let kept = |value: Option<&str>| {
            value
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .and_then(|value| HeaderValue::from_str(value).ok())
        };
        Forwarded {
            forwarded_for: incoming
                .forwarded_for
                .map(str::trim)
                .filter(|chain| !chain.is_empty())
                .and_then(|chain| HeaderValue::from_str(&format!("{chain}, {peer}")).ok())
                .unwrap_or_else(|| ip_value(peer)),
            proto: kept(incoming.proto).unwrap_or_else(|| HeaderValue::from_static(proto)),
            host: kept(incoming.host).or(host),
            real_ip: ip_value(self.client(peer, incoming.forwarded_for)),
        }
    }
}

/// An address as a header value, formatted without an intermediate string
fn ip_value(ip: IpAddr) -> HeaderValue {
    use std::io::Write;

    // The longest IPv6 address is 45 characters
    let mut buffer = [0u8; 64];
    let mut cursor = std::io::Cursor::new(&mut buffer[..]);
    let _ = write!(cursor, "{ip}");
    let length = usize::try_from(cursor.position()).unwrap_or_default();
    buffer
        .get(..length)
        .and_then(|text| HeaderValue::from_bytes(text).ok())
        .unwrap_or_else(|| HeaderValue::from_static(""))
}

/// See [`TrustedProxies::forwarded`]
pub fn forwarded<'a>(
    peer: IpAddr,
    incoming: impl FnOnce() -> Incoming<'a>,
    proto: &'static str,
    host: Option<&str>,
) -> Forwarded {
    TRUSTED
        .get()
        .unwrap_or(&TrustedProxies(IpSet::default()))
        .forwarded(peer, incoming, proto, host)
}

/// Sets the trusted proxies (once, when proksi starts)
pub fn init(proxies: TrustedProxies) {
    let _ = TRUSTED.set(proxies);
}

/// The client of the request of `session` (`None` for a connection over a unix socket), see
/// [`TrustedProxies::client`]
pub fn of_session(session: &pingora::proxy::Session) -> Option<IpAddr> {
    let peer = session.client_addr()?.as_inet()?.ip();
    let forwarded_for = session
        .req_header()
        .headers
        .get(&X_FORWARDED_FOR)
        .and_then(|value| value.to_str().ok());
    Some(client_ip(peer, forwarded_for))
}

/// Whether `peer` is a trusted proxy (`server.trusted_proxies`)
pub fn is_trusted(peer: IpAddr) -> bool {
    TRUSTED.get().is_some_and(|trusted| trusted.trusts(&peer))
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
        TrustedProxies::parse(&proxies.iter().map(ToString::to_string).collect::<Vec<_>>()).unwrap()
    }

    #[test]
    fn test_parse() {
        assert!(
            TrustedProxies::parse(&["10.0.0.0/8".into(), "127.0.0.1".into(), "::1".into()]).is_ok()
        );
        assert_eq!(
            TrustedProxies::parse(&["10.0.0".into()]),
            Err(
                "trusted_proxies: \"10.0.0\" is not an address or a network (10.0.0.0/8)"
                    .to_string()
            )
        );
    }

    #[test]
    fn test_untrusted_connections_are_the_client() {
        let proxies = trusted(&["10.0.0.0/8"]);

        // Whatever they send
        assert_eq!(
            proxies.client(ip("203.0.113.9"), Some("1.2.3.4")),
            ip("203.0.113.9")
        );
        assert_eq!(
            TrustedProxies::default().client(ip("10.0.0.1"), Some("1.2.3.4")),
            ip("10.0.0.1")
        );
    }

    #[test]
    fn test_behind_trusted_proxies() {
        let proxies = trusted(&["10.0.0.0/8", "192.168.1.1"]);

        assert_eq!(
            proxies.client(ip("10.0.0.1"), Some("203.0.113.9")),
            ip("203.0.113.9")
        );
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
        assert_eq!(
            proxies.client(ip("10.0.0.1"), Some(" 2001:db8::1 ")),
            ip("2001:db8::1")
        );
        assert_eq!(
            proxies.client(ip("10.0.0.1"), Some("203.0.113.9:4711")),
            ip("203.0.113.9")
        );
        assert_eq!(
            proxies.client(ip("10.0.0.1"), Some("[2001:db8::1]:4711")),
            ip("2001:db8::1")
        );
    }

    #[test]
    fn test_without_a_usable_header() {
        let proxies = trusted(&["10.0.0.0/8"]);

        // The proxy itself
        assert_eq!(proxies.client(ip("10.0.0.1"), None), ip("10.0.0.1"));
        assert_eq!(proxies.client(ip("10.0.0.1"), Some("")), ip("10.0.0.1"));
        // Only trusted proxies: the furthest one
        assert_eq!(
            proxies.client(ip("10.0.0.1"), Some("10.0.0.2, 10.0.0.3")),
            ip("10.0.0.2")
        );
        // Garbage stops the walk: the last address that was valid
        assert_eq!(
            proxies.client(ip("10.0.0.1"), Some("unknown, 10.0.0.2")),
            ip("10.0.0.2")
        );
    }

    fn text(value: &http::HeaderValue) -> &str {
        value.to_str().unwrap()
    }

    fn headers(forwarded: &super::Forwarded) -> (&str, &str, Option<&str>, &str) {
        (
            text(&forwarded.forwarded_for),
            text(&forwarded.proto),
            forwarded.host.as_ref().map(text),
            text(&forwarded.real_ip),
        )
    }

    #[test]
    fn test_forwarded_headers_from_clients_are_replaced() {
        let proxies = trusted(&["10.0.0.0/8"]);

        // What they sent isn't even read
        let forwarded = proxies.forwarded(
            ip("203.0.113.9"),
            || unreachable!("headers read for a client that isn't a trusted proxy"),
            "http",
            Some("a.test"),
        );
        assert_eq!(
            headers(&forwarded),
            ("203.0.113.9", "http", Some("a.test"), "203.0.113.9")
        );
        let forwarded =
            proxies.forwarded(ip("2001:db8::1"), super::Incoming::default, "https", None);
        assert_eq!(
            headers(&forwarded),
            ("2001:db8::1", "https", None, "2001:db8::1")
        );
    }

    #[test]
    fn test_forwarded_headers_from_trusted_proxies_are_kept() {
        let proxies = trusted(&["10.0.0.0/8"]);
        let incoming = || super::Incoming {
            forwarded_for: Some("203.0.113.9, 10.0.0.2"),
            proto: Some("https"),
            host: Some("www.a.test"),
        };

        assert_eq!(
            headers(&proxies.forwarded(ip("10.0.0.1"), incoming, "http", Some("a.test"))),
            (
                "203.0.113.9, 10.0.0.2, 10.0.0.1",
                "https",
                Some("www.a.test"),
                "203.0.113.9"
            )
        );
        // Without headers: as for any client
        let forwarded = proxies.forwarded(ip("10.0.0.1"), super::Incoming::default, "https", None);
        assert_eq!(headers(&forwarded), ("10.0.0.1", "https", None, "10.0.0.1"));
    }
}
