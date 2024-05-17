use std::{borrow::Cow, sync::Arc};

use dashmap::DashMap;
use pingora_load_balancing::{selection::RoundRobin, LoadBalancer};

/// A store for routes that is updated in a background thread
pub type RouteStore = Arc<DashMap<Cow<'static, str>, Arc<LoadBalancer<RoundRobin>>>>;
// impl RouteStore {
//     pub fn new() -> Self {
//         RouteStore {
//             routes: HashMap::new(),
//         }
//     }

//     pub fn get_route_keys(&self) -> Vec<String> {
//         self.routes.keys().cloned().collect()
//     }

//     /// Adds a new route using a hostname and a LoadBalancer instance wrapped in an `Arc`
//     pub fn add_route(&mut self, route: String, upstream: Arc<LoadBalancer<RoundRobin>>) {
//         self.routes.insert(route, upstream);
//     }

//     /// Gets a route from the store
//     pub fn get_route(&self, route: &str) -> Option<Arc<LoadBalancer<RoundRobin>>> {
//         self.routes.get(route).cloned()
//     }

//     /// Optimistically removes a route from the store
//     pub fn _remove_route(&mut self, route: &str) -> bool {
//         self.routes.remove_entry(route);

//         true
//     }
// }
