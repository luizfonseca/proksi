//! Traffic splits: shares of a location's clients sent to upstreams of their own. A client's
//! key (a cookie, its address) is hashed into a bucket from 0 to 99, and the splits take the
//! buckets in order: with `v2` at 10% and `v3` at 5%, buckets 0-9 go to `v2`, 10-14 to `v3`,
//! the rest to the location's upstreams. Raising a split's percentage only adds clients to it.

use std::{net::IpAddr, sync::Arc};

use pingora::lb::{selection::RoundRobin, LoadBalancer};

use crate::config::host::{Sticky, TrafficSplit};

/// A split, ready to take requests
pub struct Split {
    pub name: String,
    pub sticky: Sticky,
    /// The cookie of `sticky = "cookie"`
    pub cookie: String,
    pub load_balancer: Arc<LoadBalancer<RoundRobin>>,
    /// Its buckets: `start..end`
    start: u8,
    end: u8,
    /// The index of its first upstream among the location's
    pub first: usize,
    pub len: usize,
}

impl Split {
    /// The splits of a location of `host` at `path`, whose own upstreams number `own`
    pub fn build(
        host: &str,
        path: &str,
        own: usize,
        splits: &[TrafficSplit],
        health_check: Option<crate::config::host::HealthCheck>,
    ) -> Vec<Split> {
        let mut start = 0u8;
        let mut first = own;
        splits
            .iter()
            .map(|split| {
                let mut load_balancer =
                    crate::routing::build::load_balancer_at(&split.upstreams, first);
                crate::routing::build::set_health_check(&mut load_balancer, health_check);
                let end = start.saturating_add(split.percent).min(100);
                let built = Split {
                    name: split.name.clone(),
                    sticky: split.sticky,
                    cookie: split
                        .cookie
                        .clone()
                        .unwrap_or_else(|| default_cookie(host, path)),
                    load_balancer: Arc::new(load_balancer),
                    start,
                    end,
                    first,
                    len: split.upstreams.len(),
                };
                start = end;
                first += split.upstreams.len();
                built
            })
            .collect()
    }

    /// The share of clients it takes
    pub fn percent(&self) -> u8 {
        self.end - self.start
    }

    /// Whether the split holds the upstream at `index` among the location's
    pub fn holds(&self, index: usize) -> bool {
        (self.first..self.first + self.len).contains(&index)
    }
}

/// A cookie name of its own for a location, telling nothing about it: `_t` and 8 hex digits
pub fn default_cookie(host: &str, path: &str) -> String {
    let hash = fnv1a(
        host.as_bytes(),
        fnv1a(b"\0", fnv1a(path.as_bytes(), FNV_OFFSET)),
    );
    format!("_t{:08x}", hash & 0xffff_ffff)
}

/// What a client is known by
pub enum Key<'a> {
    Cookie(&'a str),
    Ip(IpAddr),
    /// Chosen anew for each request
    Random,
}

/// The split a client with `key_of(split)` gets, if any: the first whose buckets hold the key's
pub fn choose<'a, 'k>(
    splits: &'a [Split],
    key_of: impl Fn(&Split) -> Key<'k>,
) -> Option<&'a Split> {
    splits.iter().find(|split| {
        let bucket = bucket(&key_of(split));
        (split.start..split.end).contains(&bucket)
    })
}

fn bucket(key: &Key<'_>) -> u8 {
    let hash = match key {
        Key::Cookie(value) => fnv1a(value.as_bytes(), FNV_OFFSET),
        Key::Ip(IpAddr::V4(ip)) => fnv1a(&ip.octets(), FNV_OFFSET),
        Key::Ip(IpAddr::V6(ip)) => fnv1a(&ip.octets(), FNV_OFFSET),
        Key::Random => uuid::Uuid::new_v4().as_u64_pair().0,
    };
    u8::try_from(hash % 100).unwrap_or(0)
}

/// An upstream for the request: of the split chosen by a rule or by the client's key, unless
/// none of the split's is up, else of the location
pub async fn select(
    session: &pingora::proxy::Session,
    ctx: &mut crate::proxy_server::https_proxy::RouterContext,
    location: &crate::stores::routes::RouteStoreContainer,
) -> Option<pingora::lb::Backend> {
    use crate::routing::balance::{pick, pick_or_resolve};

    let (balancing, tried) = (location.load_balancing, ctx.tried);
    if let Some(split) = chosen(session, ctx, location) {
        // Only its healthy upstreams: if none is, the location's take the request
        if let Some(backend) = pick(&split.load_balancer, balancing, tried) {
            return Some(backend);
        }
        tracing::debug!(
            split = split.name,
            "traffic split without upstreams up: the location's instead"
        );
    }
    pick_or_resolve(&location.load_balancer, balancing, tried).await
}

fn chosen<'a>(
    session: &pingora::proxy::Session,
    ctx: &mut crate::proxy_server::https_proxy::RouterContext,
    location: &'a crate::stores::routes::RouteStoreContainer,
) -> Option<&'a Split> {
    if location.splits.is_empty() {
        return None;
    }
    if let Some(name) = &ctx.traffic_split {
        // A split of another location is no split here
        return location.splits.iter().find(|split| *split.name == **name);
    }

    // Clients without an id get a new one, set on the response
    for split in location
        .splits
        .iter()
        .filter(|split| split.sticky == Sticky::Cookie)
    {
        if cookie(session, &split.cookie).is_none()
            && !ctx
                .split_cookies
                .iter()
                .any(|(name, _)| *name == split.cookie)
        {
            let id = uuid::Uuid::new_v4().simple().to_string();
            ctx.split_cookies.push((split.cookie.clone(), id));
        }
    }

    let new_ids = &ctx.split_cookies;
    choose(&location.splits, |split| match split.sticky {
        Sticky::Cookie => cookie(session, &split.cookie)
            .or_else(|| {
                new_ids
                    .iter()
                    .find(|(name, _)| *name == split.cookie)
                    .map(|(_, id)| id.as_str())
            })
            .map_or(Key::Random, Key::Cookie),
        Sticky::ClientIp => {
            crate::proxy_server::client::of_session(session).map_or(Key::Random, Key::Ip)
        }
        Sticky::None => Key::Random,
    })
}

/// The value of the cookie `name` of the request
fn cookie<'a>(session: &'a pingora::proxy::Session, name: &str) -> Option<&'a str> {
    session
        .req_header()
        .headers
        .get_all(http::header::COOKIE)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .flat_map(|value| value.split(';'))
        .find_map(|pair| {
            let (key, value) = pair.trim().split_once('=')?;
            (key == name && !value.is_empty()).then_some(value)
        })
}

const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;

/// FNV-1a: the same hash in every instance and version, so clients keep their side
fn fnv1a(bytes: &[u8], mut hash: u64) -> u64 {
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0100_0000_01b3);
    }
    hash
}

#[cfg(test)]
mod tests {
    use crate::config::{
        host::{Sticky, TrafficSplit},
        RouteUpstream,
    };

    use super::{choose, default_cookie, Key, Split};

    fn split(name: &str, percent: u8) -> TrafficSplit {
        TrafficSplit {
            name: name.to_string(),
            percent,
            sticky: Sticky::Cookie,
            cookie: None,
            upstreams: vec![RouteUpstream {
                ip: "10.0.1.1".into(),
                port: 80,
                ..RouteUpstream::default()
            }],
        }
    }

    /// The share of `clients` keys each split gets, by name (`-` for none)
    fn shares(splits: &[Split], clients: usize) -> std::collections::BTreeMap<String, usize> {
        let mut shares = std::collections::BTreeMap::new();
        for client in 0..clients {
            let key = format!("client-{client}");
            let name = choose(splits, |_| Key::Cookie(&key))
                .map_or("-".to_string(), |split| split.name.clone());
            *shares.entry(name).or_insert(0) += 1;
        }
        shares
    }

    #[test]
    fn test_splits_take_their_share() {
        let splits = Split::build("a.test", "/", 2, &[split("v2", 10), split("v3", 5)], None);
        let shares = shares(&splits, 10_000);

        // Roughly their percentages
        assert!((800..1200).contains(&shares["v2"]), "{shares:?}");
        assert!((350..650).contains(&shares["v3"]), "{shares:?}");
        assert!((8300..8700).contains(&shares["-"]), "{shares:?}");
    }

    #[test]
    fn test_raising_a_share_only_adds_clients() {
        let before = Split::build("a.test", "/", 1, &[split("v2", 10)], None);
        let after = Split::build("a.test", "/", 1, &[split("v2", 30)], None);
        for client in 0..2_000 {
            let key = format!("client-{client}");
            if choose(&before, |_| Key::Cookie(&key)).is_some() {
                assert!(choose(&after, |_| Key::Cookie(&key)).is_some(), "{key}");
            }
        }
    }

    #[test]
    fn test_a_client_keeps_its_side() {
        let splits = Split::build("a.test", "/", 1, &[split("v2", 50)], None);
        let ip = "203.0.113.7".parse().unwrap();
        let first = choose(&splits, |_| Key::Ip(ip)).is_some();
        for _ in 0..20 {
            assert_eq!(choose(&splits, |_| Key::Ip(ip)).is_some(), first);
        }
    }

    #[test]
    fn test_upstreams_are_numbered_after_the_location_s() {
        let splits = Split::build("a.test", "/", 2, &[split("v2", 10), split("v3", 5)], None);
        assert_eq!((splits[0].first, splits[0].len), (2, 1));
        assert_eq!((splits[1].first, splits[1].len), (3, 1));
        assert!(splits[1].holds(3) && !splits[1].holds(2));
        let backends = splits[1].load_balancer.backends().get_backend();
        let backend = backends.iter().next().unwrap();
        assert_eq!(crate::routing::build::upstream_index(backend), Some(3));
    }

    #[test]
    fn test_cookie_names_tell_nothing() {
        let name = default_cookie("shop.example.com", "/checkout");
        assert!(name.starts_with("_t") && name.len() == 10, "{name}");
        assert!(!name.contains("shop") && !name.contains("split"));
        // The same everywhere, so every instance reads it
        assert_eq!(name, default_cookie("shop.example.com", "/checkout"));
        assert_ne!(name, default_cookie("shop.example.com", "/"));
    }
}
