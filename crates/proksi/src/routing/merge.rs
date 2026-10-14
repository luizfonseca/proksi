use std::collections::{BTreeMap, HashSet};

use serde::Serialize;

use crate::config::Route;

use super::{validate::validate_route, RouteSourceKind};

/// Result of merging the routes of every source
#[derive(Debug, Default)]
pub struct MergeResult {
    /// Winning route for each host (lowercase), with its source
    pub routes: BTreeMap<String, (RouteSourceKind, Route)>,
    /// Hosts defined by more than one source
    pub conflicts: Vec<Conflict>,
    /// Routes that were skipped
    pub invalid: Vec<InvalidRoute>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Conflict {
    pub host: String,
    pub winner: RouteSourceKind,
    /// Other sources defining the same host, by priority (highest first)
    pub shadowed: Vec<RouteSourceKind>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct InvalidRoute {
    pub source: RouteSourceKind,
    pub host: String,
    pub error: String,
}

/// Merges the routes of every source: for each host, the route of the source with
/// the highest priority wins. Invalid routes are skipped.
pub fn merge(snapshots: &BTreeMap<RouteSourceKind, Vec<Route>>) -> MergeResult {
    let mut result = MergeResult::default();
    // Sources defining each host, highest priority first
    let mut sources_by_host: BTreeMap<String, Vec<RouteSourceKind>> = BTreeMap::new();

    // Highest priority first, so the first valid route for a host wins
    for (source, routes) in snapshots.iter().rev() {
        let mut seen_in_source = HashSet::new();

        for route in routes {
            let host = route.host.to_lowercase();

            if let Err(error) = validate_route(route) {
                result.invalid.push(InvalidRoute {
                    source: *source,
                    host,
                    error,
                });
                continue;
            }

            if !seen_in_source.insert(host.clone()) {
                result.invalid.push(InvalidRoute {
                    source: *source,
                    host,
                    error: "duplicate host".to_string(),
                });
                continue;
            }

            sources_by_host
                .entry(host.clone())
                .or_default()
                .push(*source);
            result
                .routes
                .entry(host)
                .or_insert_with(|| (*source, route.clone()));
        }
    }

    for (host, sources) in sources_by_host {
        if let Some((winner, shadowed)) = sources.split_first() {
            if !shadowed.is_empty() {
                result.conflicts.push(Conflict {
                    host,
                    winner: *winner,
                    shadowed: shadowed.to_vec(),
                });
            }
        }
    }

    result
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use crate::routing::{test_utils::route, RouteSourceKind};

    use super::{merge, Conflict, InvalidRoute};

    fn upstream_of(result: &super::MergeResult, host: &str) -> (RouteSourceKind, String) {
        let (source, route) = &result.routes[host];
        (*source, route.upstreams[0].ip.to_string())
    }

    #[test]
    fn test_merge_single_source() {
        let snapshots = BTreeMap::from([(
            RouteSourceKind::File,
            vec![route("a.com", "10.0.0.1"), route("b.com", "10.0.0.2")],
        )]);

        let result = merge(&snapshots);

        assert_eq!(result.routes.len(), 2);
        assert_eq!(
            upstream_of(&result, "a.com"),
            (RouteSourceKind::File, "10.0.0.1".into())
        );
        assert_eq!(
            upstream_of(&result, "b.com"),
            (RouteSourceKind::File, "10.0.0.2".into())
        );
        assert!(result.conflicts.is_empty());
        assert!(result.invalid.is_empty());
    }

    #[test]
    fn test_merge_store_wins_over_file_and_docker() {
        let snapshots = BTreeMap::from([
            (RouteSourceKind::File, vec![route("a.com", "10.0.0.1")]),
            (RouteSourceKind::Store, vec![route("a.com", "10.0.0.2")]),
            (RouteSourceKind::Docker, vec![route("a.com", "10.0.0.3")]),
        ]);

        let result = merge(&snapshots);

        assert_eq!(
            upstream_of(&result, "a.com"),
            (RouteSourceKind::Store, "10.0.0.2".into())
        );
        assert_eq!(
            result.conflicts,
            vec![Conflict {
                host: "a.com".into(),
                winner: RouteSourceKind::Store,
                shadowed: vec![RouteSourceKind::Docker, RouteSourceKind::File],
            }]
        );
    }

    #[test]
    fn test_merge_falls_back_when_winner_is_removed() {
        let mut snapshots = BTreeMap::from([
            (RouteSourceKind::File, vec![route("a.com", "10.0.0.1")]),
            (RouteSourceKind::Store, vec![route("a.com", "10.0.0.2")]),
        ]);
        snapshots.insert(RouteSourceKind::Store, vec![]);

        let result = merge(&snapshots);

        assert_eq!(
            upstream_of(&result, "a.com"),
            (RouteSourceKind::File, "10.0.0.1".into())
        );
        assert!(result.conflicts.is_empty());
    }

    #[test]
    fn test_merge_skips_invalid_routes() {
        let snapshots = BTreeMap::from([(
            RouteSourceKind::Store,
            vec![route("a.com", ""), route("b.com", "10.0.0.2")],
        )]);

        let result = merge(&snapshots);

        assert!(!result.routes.contains_key("a.com"));
        assert!(result.routes.contains_key("b.com"));
        assert_eq!(
            result.invalid,
            vec![InvalidRoute {
                source: RouteSourceKind::Store,
                host: "a.com".into(),
                error: "upstreams[0].ip cannot be empty".into(),
            }]
        );
    }

    #[test]
    fn test_merge_invalid_route_falls_back_to_lower_priority_source() {
        let snapshots = BTreeMap::from([
            (RouteSourceKind::File, vec![route("a.com", "10.0.0.1")]),
            (RouteSourceKind::Store, vec![route("a.com", "")]),
        ]);

        let result = merge(&snapshots);

        assert_eq!(
            upstream_of(&result, "a.com"),
            (RouteSourceKind::File, "10.0.0.1".into())
        );
        assert_eq!(result.invalid.len(), 1);
    }

    #[test]
    fn test_merge_duplicate_host_in_one_source_keeps_the_first() {
        let snapshots = BTreeMap::from([(
            RouteSourceKind::File,
            vec![route("a.com", "10.0.0.1"), route("a.com", "10.0.0.2")],
        )]);

        let result = merge(&snapshots);

        assert_eq!(
            upstream_of(&result, "a.com"),
            (RouteSourceKind::File, "10.0.0.1".into())
        );
        assert_eq!(
            result.invalid,
            vec![InvalidRoute {
                source: RouteSourceKind::File,
                host: "a.com".into(),
                error: "duplicate host".into(),
            }]
        );
    }

    #[test]
    fn test_merge_hosts_are_case_insensitive() {
        let snapshots = BTreeMap::from([
            (
                RouteSourceKind::File,
                vec![route("Example.com", "10.0.0.1")],
            ),
            (
                RouteSourceKind::Store,
                vec![route("example.COM", "10.0.0.2")],
            ),
        ]);

        let result = merge(&snapshots);

        assert_eq!(result.routes.len(), 1);
        assert_eq!(
            upstream_of(&result, "example.com"),
            (RouteSourceKind::Store, "10.0.0.2".into())
        );
        assert_eq!(result.conflicts.len(), 1);
    }
}
