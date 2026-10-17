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
    /// Timeouts of the location's requests, instead of `server.timeouts`
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeouts: Option<LocationTimeouts>,
    /// How the path sent to upstreams is changed
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rewrite: Option<PathRewrite>,
}

/// `rewrite { ... }` in a location: the path sent to upstreams, instead of the request's
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PathRewrite {
    /// Removed from the start of the path (only at a segment boundary: `/api` from `/api/users`,
    /// not from `/apis`)
    pub strip_prefix: Option<String>,
    /// Added to the start of the path, after `strip_prefix`
    pub add_prefix: Option<String>,
}

impl PathRewrite {
    /// Whether the settings are valid: prefixes are paths, and at least one is set
    pub fn validate(&self) -> Result<(), String> {
        if self.strip_prefix.is_none() && self.add_prefix.is_none() {
            return Err("`rewrite` needs `strip_prefix` or `add_prefix`".to_string());
        }
        for (name, prefix) in [("strip_prefix", &self.strip_prefix), ("add_prefix", &self.add_prefix)] {
            if let Some(prefix) = prefix {
                if !prefix.starts_with('/') || prefix.contains(['?', '#']) {
                    return Err(format!("`{name}` must be a path starting with `/`, not {prefix:?}"));
                }
            }
        }
        Ok(())
    }

    /// The path (and query) sent to upstreams for `path` and `query`
    pub fn apply(&self, path: &str, query: Option<&str>) -> String {
        let mut path = path;
        if let Some(prefix) = self.strip_prefix.as_deref().map(|p| p.trim_end_matches('/')) {
            if let Some(rest) = path.strip_prefix(prefix) {
                if rest.is_empty() || rest.starts_with('/') {
                    path = rest;
                }
            }
        }
        let mut rewritten = match self.add_prefix.as_deref().map(|p| p.trim_end_matches('/')) {
            Some(prefix) if path.is_empty() || path == "/" => {
                if prefix.is_empty() { "/".to_string() } else { prefix.to_string() }
            }
            Some(prefix) => format!("{prefix}{path}"),
            None if path.is_empty() => "/".to_string(),
            None => path.to_string(),
        };
        if let Some(query) = query {
            rewritten.push('?');
            rewritten.push_str(query);
        }
        rewritten
    }
}

/// Timeouts of a location's requests (`timeouts { ... }` in a host or location), in seconds,
/// overriding the ones of `server.timeouts`
// Named like `server.timeouts`
#[allow(clippy::struct_field_names)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LocationTimeouts {
    /// How long each read of a request body (or websocket message) can wait
    pub read_secs: Option<u64>,
    /// How long each write of a response can wait for the client to read
    pub write_secs: Option<u64>,
}

impl LocationTimeouts {
    /// These timeouts, with the ones they don't set taken from `defaults` (the host's)
    pub fn or(self, defaults: Option<Self>) -> Self {
        let defaults = defaults.unwrap_or_default();
        Self {
            read_secs: self.read_secs.or(defaults.read_secs),
            write_secs: self.write_secs.or(defaults.write_secs),
        }
    }
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
            timeouts: None,
            rewrite: None,
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

    #[test]
    fn test_rewrites_the_path() {
        let rewrite = |strip: Option<&str>, add: Option<&str>| super::PathRewrite {
            strip_prefix: strip.map(ToString::to_string),
            add_prefix: add.map(ToString::to_string),
        };
        let strip = rewrite(Some("/api"), None);
        assert_eq!(strip.apply("/api/users", Some("page=2")), "/users?page=2");
        assert_eq!(strip.apply("/api", None), "/");
        assert_eq!(strip.apply("/api/", None), "/");
        // Only at a segment boundary
        assert_eq!(strip.apply("/apis", None), "/apis");
        assert_eq!(strip.apply("/other", None), "/other");
        // A trailing slash in the prefix is the same prefix
        assert_eq!(rewrite(Some("/api/"), None).apply("/api/users", None), "/users");

        let both = rewrite(Some("/api"), Some("/v2"));
        assert_eq!(both.apply("/api/users", None), "/v2/users");
        assert_eq!(both.apply("/api", Some("x=1")), "/v2?x=1");
        assert_eq!(rewrite(None, Some("/v2/")).apply("/users", None), "/v2/users");
        assert_eq!(rewrite(None, Some("/")).apply("/users", None), "/users");
    }

    #[test]
    fn test_invalid_rewrites() {
        let rewrite = |strip: Option<&str>, add: Option<&str>| super::PathRewrite {
            strip_prefix: strip.map(ToString::to_string),
            add_prefix: add.map(ToString::to_string),
        };
        assert!(rewrite(None, None).validate().is_err());
        assert!(rewrite(Some("api"), None).validate().is_err());
        assert!(rewrite(None, Some("/v2?x=1")).validate().is_err());
        assert!(rewrite(Some("/api"), Some("/v2")).validate().is_ok());
    }
}
