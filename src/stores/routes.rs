use std::sync::Arc;

use dashmap::DashMap;
use pingora_load_balancing::{selection::RoundRobin, LoadBalancer};

/// A store for routes that is updated in a background thread
pub type RouteStore = Arc<DashMap<String, Arc<LoadBalancer<RoundRobin>>>>;
