//! The hot path of a request allocates nothing: finding its host and location, running the
//! rules on it, choosing its traffic split and its upstream. Tests count the allocations of
//! each, with an allocator counting those of the thread measured.

use std::{
    alloc::{GlobalAlloc, Layout},
    cell::Cell,
};

use mimalloc::MiMalloc;

thread_local! {
    /// The allocations made on this thread while measured
    static COUNT: Cell<Option<usize>> = const { Cell::new(None) };
}

/// The allocator of the tests: mimalloc, counting
pub struct Counting;

fn count() {
    let _ = COUNT.try_with(|count| {
        if let Some(allocations) = count.get() {
            count.set(Some(allocations + 1));
        }
    });
}

// SAFETY: each call goes to mimalloc as it came
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        count();
        // SAFETY: as `GlobalAlloc::alloc`'s caller guarantees
        unsafe { MiMalloc.alloc(layout) }
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        count();
        // SAFETY: as `GlobalAlloc::alloc_zeroed`'s caller guarantees
        unsafe { MiMalloc.alloc_zeroed(layout) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        count();
        // SAFETY: as `GlobalAlloc::realloc`'s caller guarantees
        unsafe { MiMalloc.realloc(ptr, layout, new_size) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        // SAFETY: as `GlobalAlloc::dealloc`'s caller guarantees
        unsafe { MiMalloc.dealloc(ptr, layout) }
    }
}

/// Asserts that `run` allocates nothing, once warm (lazy statics and caches filled)
pub fn assert_no_allocations<T>(what: &str, mut run: impl FnMut() -> T) {
    run();
    let (count, _) = allocations(&mut run);
    assert_eq!(count, 0, "{what} allocated {count} times");
}

/// How many allocations `run` makes (on this thread), and what it returns
pub fn allocations<T>(run: impl FnOnce() -> T) -> (usize, T) {
    COUNT.with(|count| count.set(Some(0)));
    let value = run();
    let allocations = COUNT.with(|count| count.replace(None)).unwrap_or_default();
    (allocations, value)
}

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
