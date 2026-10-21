//! Sets of IP addresses and networks (`ip_filter`, the rules' `client`, `trusted_proxies`),
//! checked in a time that barely grows with their size: the networks are merged into sorted
//! ranges of addresses that don't overlap, and an address is found with a binary search, about
//! 17 comparisons for 100,000 networks. IPv4 is kept as IPv4-mapped IPv6 (`::ffff:a.b.c.d`), so
//! an IPv4 client connected over IPv6 is the same address.

use std::net::IpAddr;

use ipnet::IpNet;

#[derive(Debug, Default, Clone, PartialEq)]
pub struct IpSet {
    /// First and last address of each range, sorted
    ranges: Vec<(u128, u128)>,
}

impl IpSet {
    pub fn new(networks: impl IntoIterator<Item = IpNet>) -> Self {
        let mut ranges = networks.into_iter().map(range).collect::<Vec<_>>();
        ranges.sort_unstable();
        let mut merged: Vec<(u128, u128)> = Vec::with_capacity(ranges.len());
        for (first, last) in ranges {
            match merged.last_mut() {
                // Overlapping, or right after the previous one
                Some(previous) if first <= previous.1.saturating_add(1) => {
                    previous.1 = previous.1.max(last);
                }
                _ => merged.push((first, last)),
            }
        }
        merged.shrink_to_fit();
        Self { ranges: merged }
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.ranges.is_empty()
    }

    #[must_use]
    pub fn contains(&self, ip: IpAddr) -> bool {
        self.contains_address(address(ip))
    }

    /// Whether the set holds `address` (as [`address`] gives it)
    #[inline]
    #[must_use]
    pub fn contains_address(&self, address: u128) -> bool {
        // The ranges starting at or before it: the last of them may hold it
        let after = self.ranges.partition_point(|(first, _)| *first <= address);
        after
            .checked_sub(1)
            .and_then(|index| self.ranges.get(index))
            .is_some_and(|(_, last)| address <= *last)
    }
}

/// An address or a network, as written in the configuration (`10.0.0.1`, `10.0.0.0/8`)
#[must_use]
pub fn network(entry: &str) -> Option<IpNet> {
    entry
        .parse::<IpNet>()
        .or_else(|_| entry.parse::<IpAddr>().map(IpNet::from))
        .ok()
}

/// An address over IPv6, IPv4 as IPv4-mapped
#[inline]
#[must_use]
pub fn address(ip: IpAddr) -> u128 {
    match ip {
        IpAddr::V4(v4) => u128::from(v4.to_ipv6_mapped()),
        IpAddr::V6(v6) => u128::from(v6),
    }
}

/// The first and last address of a network
fn range(network: IpNet) -> (u128, u128) {
    (address(network.network()), address(network.broadcast()))
}

#[cfg(test)]
mod tests {
    use std::net::IpAddr;

    use ipnet::IpNet;

    use super::{network, IpSet};

    fn set(entries: &[&str]) -> IpSet {
        IpSet::new(entries.iter().map(|entry| network(entry).unwrap()))
    }

    fn ip(text: &str) -> IpAddr {
        text.parse().unwrap()
    }

    #[test]
    fn test_addresses_and_networks() {
        let set = set(&[
            "10.0.0.0/8",
            "203.0.113.9",
            "2001:db8::/32",
            "192.168.1.0/24",
        ]);
        assert!(set.contains(ip("10.1.2.3")));
        assert!(set.contains(ip("203.0.113.9")));
        assert!(!set.contains(ip("203.0.113.10")));
        assert!(set.contains(ip("2001:db8::1")));
        assert!(!set.contains(ip("2001:db9::1")));
        assert!(!set.contains(ip("11.0.0.0")));
        // IPv4 over IPv6 is the same client
        assert!(set.contains(ip("::ffff:10.0.0.1")));
        assert!(!IpSet::default().contains(ip("10.0.0.1")));
    }

    #[test]
    fn test_overlapping_networks_are_merged() {
        let set = set(&["10.0.0.0/8", "10.1.0.0/16", "11.0.0.0/8", "0.0.0.0/32"]);
        // `10.0.0.0/8` and `11.0.0.0/8` are next to each other: one range
        assert_eq!(set.ranges.len(), 2);
        assert!(set.contains(ip("11.255.255.255")));
        assert!(set.contains(ip("0.0.0.0")));
        assert!(!set.contains(ip("12.0.0.0")));
    }

    #[test]
    fn test_the_same_as_checking_each_network() {
        // Pseudo-random networks of every size, against pseudo-random addresses
        let mut seed = 0x2545_f491_4f6c_dd1d_u64;
        let mut next = move || {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            seed
        };
        let networks = (0..2000)
            .map(|_| {
                let bits = next();
                let length = u8::try_from(8 + bits % 25).unwrap();
                IpNet::new(
                    IpAddr::from(u32::try_from(bits >> 32).unwrap().to_be_bytes()),
                    length,
                )
                .unwrap()
                .trunc()
            })
            .collect::<Vec<_>>();
        let set = IpSet::new(networks.iter().copied());
        for _ in 0..20_000 {
            let ip = IpAddr::from(u32::try_from(next() >> 32).unwrap().to_be_bytes());
            let expected = networks.iter().any(|network| network.contains(&ip));
            assert_eq!(set.contains(ip), expected, "{ip}");
        }
    }

    #[test]
    fn test_the_same_as_checking_each_ipv6_network() {
        let mut seed = 0x9e37_79b9_7f4a_7c15_u64;
        let mut next = move || {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            seed
        };
        // Addresses close to each other (a shared /32), so networks overlap and nest
        let address = |bits: u64| {
            let low = u128::from(bits & 0xffff_ffff_ffff);
            IpAddr::from((0x2001_0db8_u128 << 96 | low).to_be_bytes())
        };
        let networks = (0..2000)
            .map(|_| {
                let bits = next();
                let length = u8::try_from(80 + bits % 49).unwrap();
                IpNet::new(address(next()), length).unwrap().trunc()
            })
            .collect::<Vec<_>>();
        let set = IpSet::new(networks.iter().copied());
        for _ in 0..20_000 {
            let ip = address(next());
            let expected = networks.iter().any(|network| network.contains(&ip));
            assert_eq!(set.contains(ip), expected, "{ip}");
        }
    }
}
