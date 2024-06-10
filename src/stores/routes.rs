use std::{borrow::Cow, collections::HashMap, sync::Arc};

use dashmap::DashMap;
use http::{HeaderName, HeaderValue};
use path_tree::PathTree;
use pingora_load_balancing::{selection::RoundRobin, LoadBalancer};

use crate::config::RoutePlugin;

#[derive(Debug, Default, Clone)]
pub struct RouteStorePathMatcher {
    pub prefix: Option<String>,
    pub suffix: Option<String>,
    pub pattern: Option<PathTree<usize>>,
}

impl RouteStorePathMatcher {
    pub fn new() -> Self {
        RouteStorePathMatcher::default()
    }

    // From a given list of patterns, generate a tree structure
    // to match against incoming requests
    pub fn with_pattern(&mut self, pattern: &[Cow<'_, str>]) -> &mut Self {
        if pattern.is_empty() {
            return self;
        }

        let mut path_tree = PathTree::new();
        for (index, value) in pattern.iter().enumerate() {
            let _ = path_tree.insert(value, index);
        }

        self.pattern = Some(path_tree);
        self
    }
}

#[derive(Clone)]
pub struct RouteStoreContainer {
    pub load_balancer: Arc<LoadBalancer<RoundRobin>>,
    pub path_matcher: RouteStorePathMatcher,
    pub host_header_remove: Vec<String>,
    pub host_header_add: Vec<(HeaderName, HeaderValue)>,

    pub self_signed_certificate: bool,

    pub plugins: HashMap<String, RoutePlugin>,
}

impl RouteStoreContainer {
    pub fn new(load_balancer: LoadBalancer<RoundRobin>) -> Self {
        RouteStoreContainer {
            load_balancer: Arc::new(load_balancer),
            path_matcher: RouteStorePathMatcher::new(),
            host_header_remove: Vec::new(),
            host_header_add: Vec::new(),
            self_signed_certificate: false,
            plugins: HashMap::new(),
        }
    }
}

// LoadBalancer<RoundRobin>
/// A store for routes that is updated in a background thread
pub type RouteStore = Arc<DashMap<String, RouteStoreContainer>>;

#[cfg(test)]

mod tests {

    use super::*;

    #[test]
    fn test_router_container_defaults_empty_pattern() {
        let load_balancer = LoadBalancer::<RoundRobin>::try_from_iter(vec!["1.1.1.1:80"]).unwrap();
        let route_store = RouteStoreContainer::new(load_balancer);

        assert_eq!(route_store.path_matcher.pattern.is_none(), true);
    }

    #[test]
    fn test_router_container_works_with_valid_and_invalid_pattern() {
        let load_balancer = LoadBalancer::<RoundRobin>::try_from_iter(vec!["1.1.1.1:80"]).unwrap();
        let mut route_store = RouteStoreContainer::new(load_balancer);
        route_store
            .path_matcher
            .with_pattern(&[Cow::Borrowed("/auth")]);

        assert_eq!(route_store.path_matcher.pattern.is_some(), true);

        let pattern = route_store.path_matcher.pattern.as_ref().unwrap();
        assert_eq!(pattern.find("/auth").is_some(), true);

        let (h, p) = pattern.find("/auth").unwrap();
        assert_eq!(h, &0);
        assert_eq!(p.pattern(), "/auth");

        assert_eq!(pattern.find("/invalid").is_none(), true);
    }
}
