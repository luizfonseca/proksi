//! The hot path of a request allocates nothing: finding its host and location, running the
//! rules on it, choosing its traffic split and its upstream. Tests count the allocations of
//! each, with the counting allocator of `proksi_plugins`.

pub use proksi_plugins::allocations::{allocations, assert_no_allocations, Counting};

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use crate::{
        config::host::{LoadBalancing, Location},
        routing::{
            balance::{pick, Tried},
            build::build_host_container,
            paths::normalize_path,
            split::{choose, Key, Split},
            test_utils::host,
        },
        stores,
    };

    use super::{allocations, assert_no_allocations};

    #[test]
    fn test_the_allocator_counts() {
        let (count, _) = allocations(|| vec![1u8; 16]);
        assert_eq!(count, 1);
        let (count, ()) = allocations(|| {});
        assert_eq!(count, 0);
    }

    #[test]
    fn test_finding_the_host_and_location_allocates_nothing() {
        let mut config = host("hot-path.test", "10.0.0.1");
        let location = config.locations[0].clone();
        for path in ["/api", "/users/:id", "/files/*"] {
            config.locations.push(Location {
                path: path.to_string(),
                exact: false,
                ..location.clone()
            });
        }
        let container = Arc::new(build_host_container(&config).unwrap());
        stores::insert_route("hot-path.test".to_string(), container);

        assert_no_allocations("finding a host", || {
            stores::lookup::find_host("hot-path.test", |_| true).unwrap()
        });
        assert_no_allocations("finding no host", || {
            stores::lookup::find_host("unknown.hot-path.test", |_| true)
        });
        assert_no_allocations("the TLS name check", || {
            stores::lookup::named_host("hot-path.test")
        });
        let found = stores::lookup::find_host("hot-path.test", |_| true).unwrap();
        for path in ["/api/orders", "/users/42", "/files/a/b.txt", "/"] {
            assert_no_allocations(path, || {
                let path = normalize_path(path);
                found.location_for(&path).is_some()
            });
        }
        stores::remove_route("hot-path.test");
    }

    #[test]
    fn test_matching_regex_locations_allocates_nothing() {
        let matcher = crate::routing::paths::LocationMatcher::new([
            vec!["^/blog/[a-z]{8}/(posts|drafts)$".to_string()],
            vec!["/*".to_string()],
        ]);
        assert_no_allocations("regex location", || {
            matcher.find("/blog/abcdefgh/posts");
            matcher.find("/static/site.css");
        });
    }

    #[test]
    fn test_checking_ip_sets_allocates_nothing() {
        let networks = (0..1000)
            .filter_map(|i| crate::ip_set::network(&format!("10.{}.{}.0/24", i / 256, i % 256)));
        let set = crate::ip_set::IpSet::new(networks);
        let client = "10.3.4.5".parse().unwrap();
        assert_no_allocations("an IP set", || set.contains(client));
    }

    #[test]
    fn test_choosing_a_split_and_an_upstream_allocates_nothing() {
        let split: crate::config::host::TrafficSplit = serde_json::from_value(serde_json::json!({
            "name": "v2",
            "percent": 50,
            "upstreams": [{ "ip": "10.0.1.1", "port": 80 }],
        }))
        .unwrap();
        let splits = Split::build("hot-path.test", "/", 1, &[split], None);
        assert_no_allocations("choosing a split", || {
            choose(&splits, |_| Key::Cookie("client-1")).is_some()
        });

        let container = build_host_container(&host("hot-path.test", "10.0.0.1")).unwrap();
        let load_balancer = &container.locations[0].load_balancer;
        let turn = crate::routing::balance::Turn::default();
        let mut tried = Tried::default();
        tried.add(0);
        for (balancing, tried) in [
            (LoadBalancing::RoundRobin, Tried::default()),
            (LoadBalancing::LeastConnections, Tried::default()),
            (LoadBalancing::RoundRobin, tried),
            (LoadBalancing::LeastConnections, tried),
        ] {
            assert_no_allocations(&format!("picking an upstream ({balancing:?})"), || {
                pick(load_balancer, &turn, balancing, tried)
            });
        }
    }
}
