use std::{borrow::Cow, sync::Arc};

use dashmap::DashMap;
use pingora_load_balancing::{selection::RoundRobin, LoadBalancer};

/// A store for routes that is updated in a background thread
pub type RouteStore = Arc<DashMap<Cow<'static, str>, Arc<LoadBalancer<RoundRobin>>>>;
