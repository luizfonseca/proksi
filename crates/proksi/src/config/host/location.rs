use serde::{Deserialize, Serialize};

use crate::config::{RouteCache, RouteHeader, RoutePlugin, RouteUpstream};

/// Requests of a host matching a path, and how they are handled
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Location {
    /// Path pattern (`/api`, `/users/:id`, `/*`)
    pub path: String,
    /// Match only the path itself instead of the path and everything below it.
    /// Set for routes converted from `routes = [...]`, which matched exactly.
    #[serde(default)]
    pub exact: bool,
    #[serde(default)]
    pub upstreams: Vec<RouteUpstream>,
    pub headers: Option<RouteHeader>,
    pub plugins: Option<Vec<RoutePlugin>>,
    pub cache: Option<RouteCache>,
}

impl Location {
    /// Path patterns matched by this location: the path and everything below it
    /// (`/api` matches `/api`, `/api/` and `/api/users`, not `/apis`)
    pub fn patterns(&self) -> Vec<String> {
        if self.exact || self.path.ends_with(['*', '+']) {
            return vec![self.path.clone()];
        }
        let base = self.path.trim_end_matches('/');
        vec![self.path.clone(), format!("{base}/*")]
    }
}

#[cfg(test)]
mod tests {
    use super::Location;

    fn location(path: &str, exact: bool) -> Location {
        Location {
            path: path.to_string(),
            exact,
            upstreams: vec![],
            headers: None,
            plugins: None,
            cache: None,
        }
    }

    #[test]
    fn test_location_patterns_match_the_path_and_below() {
        assert_eq!(
            location("/v1/session", false).patterns(),
            vec!["/v1/session", "/v1/session/*"]
        );
        assert_eq!(location("/", false).patterns(), vec!["/", "/*"]);
        assert_eq!(location("/api/", false).patterns(), vec!["/api/", "/api/*"]);
    }

    #[test]
    fn test_location_patterns_with_wildcards_or_exact() {
        assert_eq!(location("/*", false).patterns(), vec!["/*"]);
        assert_eq!(
            location("/files/:path*", false).patterns(),
            vec!["/files/:path*"]
        );
        assert_eq!(
            location("/files/:path+", false).patterns(),
            vec!["/files/:path+"]
        );
        assert_eq!(location("/auth", true).patterns(), vec!["/auth"]);
    }
}
