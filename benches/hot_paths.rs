//! Benchmarks of the request hot path: checking a client against IP sets (`ip_filter`, the
//! rules' `client`, `trusted_proxies`), normalizing and matching request paths to locations,
//! and testing texts against lists of values (the rules' `prefix`, `suffix`, `contains`, `in`).
//!
//! proksi is a binary crate, so the modules measured here are included from its sources.

// The modules' tests are compiled too (benchmarks build with `cfg(test)`), but not run
#![allow(dead_code, unused_imports, clippy::unwrap_used, clippy::expect_used)]

use std::{hint::black_box, net::IpAddr};

use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion};

#[path = "../crates/proksi/src/ip_set.rs"]
mod ip_set;

#[path = "../crates/proksi/src/routing/paths/mod.rs"]
mod paths;

#[path = "../crates/proksi/src/rules/text_set.rs"]
mod text_set;

/// `count` IPv4 networks, with gaps between them so none are merged
fn networks(count: u32) -> Vec<ipnet::IpNet> {
    (0..count)
        .map(|i| {
            let address = std::net::Ipv4Addr::from(0x0A00_0000_u32 + 512 * i);
            ip_set::network(&format!("{address}/24")).unwrap()
        })
        .collect()
}

fn bench_ip_set(c: &mut Criterion) {
    let mut group = c.benchmark_group("ip_set");
    let inside: IpAddr = "10.0.2.17".parse().unwrap();
    let outside: IpAddr = "203.0.113.9".parse().unwrap();
    let v6: IpAddr = "2001:db8::1".parse().unwrap();

    for count in [10_u32, 1_000, 100_000] {
        let networks = networks(count);
        group.bench_with_input(
            BenchmarkId::new("build", count),
            &networks,
            |b, networks| {
                b.iter(|| ip_set::IpSet::new(black_box(networks.iter().copied())));
            },
        );

        let set = ip_set::IpSet::new(networks);
        group.bench_with_input(BenchmarkId::new("contains", count), &set, |b, set| {
            b.iter(|| {
                (
                    set.contains(black_box(inside)),
                    set.contains(black_box(outside)),
                    set.contains(black_box(v6)),
                )
            });
        });
    }

    group.bench_function("parse_networks", |b| {
        let entries = [
            "10.0.0.0/8",
            "192.168.1.10",
            "2001:db8::/32",
            "::1",
            "172.16.0.0/12",
            "not an address",
        ];
        b.iter(|| {
            black_box(entries)
                .iter()
                .filter_map(|entry| ip_set::network(entry))
                .count()
        });
    });
    group.finish();
}

fn bench_paths(c: &mut Criterion) {
    let mut group = c.benchmark_group("paths");

    let clean = "/api/v1/users/42/orders?page=2";
    let tricky = "//api/./v1//users/../users/42/orders/";
    group.bench_function("normalize_clean", |b| {
        b.iter(|| paths::normalize_path(black_box(clean)).len());
    });
    group.bench_function("normalize_tricky", |b| {
        b.iter(|| paths::normalize_path(black_box(tricky)).len());
    });

    let patterns = |patterns: &[&str]| patterns.iter().map(ToString::to_string).collect();
    let mut locations: Vec<Vec<String>> = vec![
        patterns(&["/*"]),
        patterns(&["/v1/session", "/v1/session/*"]),
        patterns(&["/auth"]),
        patterns(&["/static/*"]),
        patterns(&["/api/:version/users/:id"]),
    ];
    for i in 0..100 {
        locations.push(vec![format!("/service-{i}"), format!("/service-{i}/*")]);
    }
    let matcher = paths::LocationMatcher::new(locations.clone());
    group.bench_function("build_matcher_105_locations", |b| {
        b.iter(|| paths::LocationMatcher::new(black_box(locations.clone())));
    });

    let requests = [
        "/",
        "/v1/session/abc",
        "/auth",
        "/static/css/app.css",
        "/api/v2/users/42",
        "/service-73/health",
        "//v1/./session/../session/x",
        "/unknown/path/here",
    ];
    group.bench_function("find_location", |b| {
        b.iter(|| {
            black_box(requests)
                .iter()
                .filter_map(|path| matcher.find(path))
                .sum::<usize>()
        });
    });
    group.finish();
}

fn values(count: usize) -> Vec<String> {
    (0..count).map(|i| format!("/probe-{i}.x")).collect()
}

fn bench_text_set(c: &mut Criterion) {
    let mut group = c.benchmark_group("text_set");
    let text = "/products/42/reviews?page=2";

    for count in [5_usize, 50, 5_000] {
        let ends = text_set::Ends::new(values(count));
        group.bench_with_input(
            BenchmarkId::new("prefix_suffix", count),
            &ends,
            |b, ends| {
                b.iter(|| (ends.starts(black_box(text)), ends.ends(black_box(text))));
            },
        );

        let exact = text_set::Exact::new(values(count));
        group.bench_with_input(BenchmarkId::new("in", count), &exact, |b, exact| {
            b.iter(|| exact.holds(black_box(text)));
        });

        let parts = text_set::Parts::new(values(count)).unwrap();
        group.bench_with_input(BenchmarkId::new("contains", count), &parts, |b, parts| {
            b.iter(|| parts.found_in(black_box(text)));
        });
    }

    group.bench_function("build_contains_5000", |b| {
        let values = values(5_000);
        b.iter(|| text_set::Parts::new(black_box(values.clone())).unwrap());
    });
    group.finish();
}

criterion_group!(benches, bench_ip_set, bench_paths, bench_text_set);
criterion_main!(benches);
