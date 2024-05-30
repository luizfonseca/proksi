use std::{borrow::Cow, sync::Arc};

use dashmap::DashMap;
use path_tree::PathTree;
use pingora_load_balancing::{selection::RoundRobin, LoadBalancer};

pub struct RouteStorePathMatcher {
    pub prefix: Option<String>,
    pub suffix: Option<String>,
    pub pattern: Option<PathTree<i32>>,
}

impl RouteStorePathMatcher {
    pub fn new() -> Self {
        RouteStorePathMatcher::default()
    }

    // From a given list of patterns, generate a tree structure
    // to match against incoming requests
    pub fn with_pattern(&mut self, pattern: Vec<Cow<'_, str>>) -> &mut Self {
        if pattern.is_empty() {
            return self;
        }

        let mut path_tree = PathTree::new();
        for (index, value) in pattern.iter().enumerate() {
            let _ = path_tree.insert(&value, index as i32);
        }

        self.pattern = Some(path_tree);
        self
    }

    pub fn build(self) -> Self {
        self
    }
}

impl Default for RouteStorePathMatcher {
    fn default() -> Self {
        Self {
            prefix: None,
            suffix: None,
            pattern: None,
        }
    }
}

pub struct RouteStoreContainer {
    pub load_balancer: Arc<LoadBalancer<RoundRobin>>,
    pub path_matcher: RouteStorePathMatcher,
}

impl RouteStoreContainer {
    pub fn new(load_balancer: Arc<LoadBalancer<RoundRobin>>) -> Self {
        RouteStoreContainer {
            load_balancer,
            path_matcher: RouteStorePathMatcher::new(),
        }
    }

    pub fn path_matcher(&mut self) -> &mut RouteStorePathMatcher {
        &mut self.path_matcher
    }
}

// LoadBalancer<RoundRobin>
/// A store for routes that is updated in a background thread
pub type RouteStore = Arc<DashMap<String, Arc<RouteStoreContainer>>>;

#[cfg(test)]

mod tests {

    use super::*;

    #[test]
    fn test_router_container_defaults_empty_pattern() {
        let load_balancer =
            Arc::new(LoadBalancer::<RoundRobin>::try_from_iter(vec!["1.1.1.1:80"]).unwrap());
        let route_store = RouteStoreContainer::new(load_balancer);

        assert_eq!(route_store.path_matcher.pattern.is_none(), true);
    }

    #[test]
    fn test_router_container_works_with_valid_and_invalid_pattern() {
        let load_balancer =
            Arc::new(LoadBalancer::<RoundRobin>::try_from_iter(vec!["1.1.1.1:80"]).unwrap());
        let mut route_store = RouteStoreContainer::new(load_balancer);
        route_store
            .path_matcher()
            .with_pattern(vec![Cow::Borrowed("/auth")]);

        assert_eq!(route_store.path_matcher.pattern.is_some(), true);

        let pattern = route_store.path_matcher.pattern.as_ref().unwrap();
        assert_eq!(pattern.find("/auth").is_some(), true);

        let (h, p) = pattern.find("/auth").unwrap();
        assert_eq!(h, &0);
        assert_eq!(p.pattern(), "/auth");

        assert_eq!(pattern.find("/invalid").is_none(), true);
    }
}
