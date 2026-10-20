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
    /// How the upstreams are checked, instead of the default
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub health_check: Option<HealthCheck>,
    /// Rules of the location's requests (ADR 0007), in order
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub rules: Vec<crate::rules::Rule>,
    /// Shares of the requests sent to other upstreams (`traffic_split "<name>" { ... }`)
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub splits: Vec<TrafficSplit>,
    /// Copies of the requests sent to other upstreams, whose answers are dropped (`mirror`)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mirror: Option<Mirror>,
    /// How each request's upstream is chosen
    #[serde(default, skip_serializing_if = "LoadBalancing::is_round_robin")]
    pub load_balancing: LoadBalancing,
    /// Requests sent to another upstream when one fails (`retry { ... }`)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retry: Option<Retry>,
}

/// How a location's upstream is chosen for each request
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LoadBalancing {
    /// In turn, in proportion to their weight
    #[default]
    RoundRobin,
    /// The one with the fewest requests in progress, for its weight
    LeastConnections,
}

impl LoadBalancing {
    #[allow(clippy::trivially_copy_pass_by_ref)]
    pub fn is_round_robin(&self) -> bool {
        *self == LoadBalancing::RoundRobin
    }
}

/// `retry { ... }`: requests sent to another upstream when one can't be reached, or answers
/// with one of the statuses of `on`
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Retry {
    /// Tries after the first
    #[serde(default = "Retry::default_attempts")]
    pub attempts: u8,
    /// `connect` (the upstream can't be reached), and statuses such as `502`
    #[serde(default = "Retry::default_on")]
    pub on: Vec<String>,
}

impl Retry {
    pub fn default_attempts() -> u8 {
        1
    }
    pub fn default_on() -> Vec<String> {
        ["connect", "502", "503", "504"]
            .map(str::to_string)
            .to_vec()
    }
}

/// `mirror { ... }`: copies of a location's requests, sent to upstreams of their own. Their
/// answers are dropped: the requests never wait on them.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Mirror {
    /// The share of requests copied, from 0 to 100 (rules can choose others)
    #[serde(default = "Mirror::default_percent")]
    pub percent: u8,
    /// Requests with a larger body aren't copied
    #[serde(default = "Mirror::default_max_body_size")]
    pub max_body_size: u64,
    /// Copies being sent at most: others are dropped
    #[serde(default = "Mirror::default_max_in_flight")]
    pub max_in_flight: usize,
    /// How long a copy can take, from connecting to its answer
    #[serde(default = "Mirror::default_timeout_secs")]
    pub timeout_secs: u64,
    pub upstreams: Vec<RouteUpstream>,
}

impl Mirror {
    pub fn default_percent() -> u8 {
        100
    }
    pub fn default_max_body_size() -> u64 {
        64 * 1024
    }
    pub fn default_max_in_flight() -> usize {
        100
    }
    pub fn default_timeout_secs() -> u64 {
        5
    }
}

/// `traffic_split "<name>" { ... }`: a share of a location's clients, sent to upstreams of its
/// own
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TrafficSplit {
    pub name: String,
    /// The share of clients, from 0 to 100
    pub percent: u8,
    /// How a client keeps its side
    #[serde(default)]
    pub sticky: Sticky,
    /// The cookie of `sticky = "cookie"` (default: a name of its own, telling nothing)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cookie: Option<String>,
    pub upstreams: Vec<RouteUpstream>,
}

/// How a client keeps its side of a traffic split
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Sticky {
    /// A random id in a cookie, set on the first response
    #[default]
    Cookie,
    /// The client's address
    ClientIp,
    /// Each request is chosen on its own
    None,
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
    /// A regular expression the path is matched against, replaced by `replace`
    #[serde(rename = "match")]
    pub pattern: Option<String>,
    /// What a path matching `match` becomes (`$1`, `${name}` for its groups)
    pub replace: Option<String>,
}

/// Compiled patterns are limited, as they can come from a store anyone with access writes to
const PATTERN_SIZE_LIMIT: usize = 1 << 20;

/// `strip_prefix` and `add_prefix` are paths
fn check_prefix(name: &str, prefix: &str) -> Result<(), String> {
    if !prefix.starts_with('/') || prefix.contains(['?', '#']) {
        return Err(format!(
            "`{name}` must be a path starting with `/`, not {prefix:?}"
        ));
    }
    Ok(())
}

impl PathRewrite {
    /// Checks the settings and compiles the pattern, once (when the location is built)
    pub fn compile(&self) -> Result<CompiledRewrite, String> {
        for (name, prefix) in [
            ("strip_prefix", &self.strip_prefix),
            ("add_prefix", &self.add_prefix),
        ] {
            if let Some(prefix) = prefix {
                check_prefix(name, prefix)?;
            }
        }
        let prefixes = self.strip_prefix.is_some() || self.add_prefix.is_some();
        let pattern = match (&self.pattern, &self.replace) {
            (None, None) if !prefixes => {
                return Err(
                    "`rewrite` needs `strip_prefix` or `add_prefix`, or `match` and `replace`"
                        .to_string(),
                );
            }
            (None, None) => None,
            (Some(_), None) | (None, Some(_)) => {
                return Err("`match` and `replace` go together".to_string());
            }
            (Some(_), Some(_)) if prefixes => {
                return Err(
                    "`match` and `replace` can't be combined with `strip_prefix` or `add_prefix`"
                        .to_string(),
                );
            }
            (Some(pattern), Some(_)) => Some(
                regex::RegexBuilder::new(pattern)
                    .size_limit(PATTERN_SIZE_LIMIT)
                    .build()
                    .map_err(|err| format!("invalid `match`: {err}"))?,
            ),
        };
        Ok(CompiledRewrite {
            settings: self.clone(),
            pattern,
        })
    }
}

/// A location's [`PathRewrite`], with its pattern compiled
#[derive(Debug, Clone)]
pub struct CompiledRewrite {
    settings: PathRewrite,
    pattern: Option<regex::Regex>,
}

/// `path` without `prefix`, removed at a segment boundary only (`/api` from `/api/users`, not
/// from `/apis`)
fn strip_segments<'a>(path: &'a str, prefix: &str) -> &'a str {
    match path.strip_prefix(prefix) {
        Some(rest) if rest.is_empty() || rest.starts_with('/') => rest,
        _ => path,
    }
}

impl CompiledRewrite {
    /// The path (and query) sent to upstreams for `path` and `query`
    pub fn apply(&self, path: &str, query: Option<&str>) -> String {
        let mut rewritten = match (&self.pattern, &self.settings.replace) {
            (Some(pattern), Some(replace)) => {
                let replaced = pattern.replace(path, replace.as_str());
                if replaced.starts_with('/') {
                    replaced.into_owned()
                } else {
                    format!("/{replaced}")
                }
            }
            _ => self.prefixes(path),
        };
        if let Some(query) = query {
            rewritten.push('?');
            rewritten.push_str(query);
        }
        rewritten
    }

    fn prefixes(&self, path: &str) -> String {
        let mut path = path;
        if let Some(prefix) = self
            .settings
            .strip_prefix
            .as_deref()
            .map(|p| p.trim_end_matches('/'))
        {
            path = strip_segments(path, prefix);
        }
        match self
            .settings
            .add_prefix
            .as_deref()
            .map(|p| p.trim_end_matches('/'))
        {
            Some(prefix) if path.is_empty() || path == "/" => {
                if prefix.is_empty() {
                    "/".to_string()
                } else {
                    prefix.to_string()
                }
            }
            Some(prefix) => format!("{prefix}{path}"),
            None if path.is_empty() => "/".to_string(),
            None => path.to_string(),
        }
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
    /// How long the whole body of a request can take to arrive, after its headers
    pub body_secs: Option<u64>,
    /// Toward the location's upstreams
    pub upstream: Option<UpstreamTimeouts>,
}

impl LocationTimeouts {
    /// These timeouts, with the ones they don't set taken from `defaults` (the host's)
    pub fn or(self, defaults: Option<Self>) -> Self {
        let defaults = defaults.unwrap_or_default();
        Self {
            read_secs: self.read_secs.or(defaults.read_secs),
            write_secs: self.write_secs.or(defaults.write_secs),
            body_secs: self.body_secs.or(defaults.body_secs),
            upstream: match self.upstream {
                Some(upstream) => Some(upstream.or(defaults.upstream)),
                None => defaults.upstream,
            },
        }
    }
}

/// `health_check { ... }` in a host, a location or a `tcp` block: a TCP connection to each
/// upstream on an interval. An upstream is unhealthy after two failed checks in a row, and gets
/// connections again after a successful one, or when no upstream is healthy.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HealthCheck {
    /// Off, upstreams are never marked unhealthy (default: on)
    pub enabled: Option<bool>,
    /// How often each upstream is checked (default: 15)
    pub interval_secs: Option<u64>,
}

impl HealthCheck {
    /// This health check, with what it doesn't set taken from `defaults` (the host's)
    pub fn or(self, defaults: Option<Self>) -> Self {
        let defaults = defaults.unwrap_or_default();
        Self {
            enabled: self.enabled.or(defaults.enabled),
            interval_secs: self.interval_secs.or(defaults.interval_secs),
        }
    }

    /// How often upstreams are checked, if they are
    pub fn interval(this: Option<Self>) -> Option<std::time::Duration> {
        let this = this.unwrap_or_default();
        this.enabled
            .unwrap_or(true)
            .then(|| std::time::Duration::from_secs(this.interval_secs.unwrap_or(15).max(1)))
    }
}

/// Timeouts toward upstreams (`upstream { ... }` in `timeouts`), in seconds
// Named like `server.timeouts`
#[allow(clippy::struct_field_names)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UpstreamTimeouts {
    /// How long to wait for an upstream to accept a connection
    pub connect_secs: Option<u64>,
    /// How long the TLS handshake with an HTTPS upstream can take, after connecting
    pub tls_secs: Option<u64>,
    /// How long each read of a response can wait, including for its headers
    pub read_secs: Option<u64>,
    /// How long each write of a request can wait for the upstream to read
    pub write_secs: Option<u64>,
    /// How long a connection is kept open between requests, to be reused
    pub idle_secs: Option<u64>,
}

impl UpstreamTimeouts {
    /// These timeouts, with the ones they don't set taken from `defaults`
    pub fn or(self, defaults: Option<Self>) -> Self {
        let defaults = defaults.unwrap_or_default();
        Self {
            connect_secs: self.connect_secs.or(defaults.connect_secs),
            tls_secs: self.tls_secs.or(defaults.tls_secs),
            read_secs: self.read_secs.or(defaults.read_secs),
            write_secs: self.write_secs.or(defaults.write_secs),
            idle_secs: self.idle_secs.or(defaults.idle_secs),
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
            health_check: None,
            rules: vec![],
            splits: vec![],
            mirror: None,
            load_balancing: crate::config::host::LoadBalancing::RoundRobin,
            retry: None,
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
        let rewrite = |strip: Option<&str>, add: Option<&str>| {
            super::PathRewrite {
                strip_prefix: strip.map(ToString::to_string),
                add_prefix: add.map(ToString::to_string),
                ..Default::default()
            }
            .compile()
            .unwrap()
        };
        let strip = rewrite(Some("/api"), None);
        assert_eq!(strip.apply("/api/users", Some("page=2")), "/users?page=2");
        assert_eq!(strip.apply("/api", None), "/");
        assert_eq!(strip.apply("/api/", None), "/");
        // Only at a segment boundary
        assert_eq!(strip.apply("/apis", None), "/apis");
        assert_eq!(strip.apply("/other", None), "/other");
        // A trailing slash in the prefix is the same prefix
        assert_eq!(
            rewrite(Some("/api/"), None).apply("/api/users", None),
            "/users"
        );

        let both = rewrite(Some("/api"), Some("/v2"));
        assert_eq!(both.apply("/api/users", None), "/v2/users");
        assert_eq!(both.apply("/api", Some("x=1")), "/v2?x=1");
        assert_eq!(
            rewrite(None, Some("/v2/")).apply("/users", None),
            "/v2/users"
        );
        assert_eq!(rewrite(None, Some("/")).apply("/users", None), "/users");
    }

    #[test]
    fn test_invalid_rewrites() {
        let rewrite = |strip: Option<&str>, add: Option<&str>| super::PathRewrite {
            strip_prefix: strip.map(ToString::to_string),
            add_prefix: add.map(ToString::to_string),
            ..Default::default()
        };
        assert!(rewrite(None, None).compile().is_err());
        assert!(rewrite(Some("api"), None).compile().is_err());
        assert!(rewrite(None, Some("/v2?x=1")).compile().is_err());
        assert!(rewrite(Some("/api"), Some("/v2")).compile().is_ok());

        let regex = |pattern: Option<&str>, replace: Option<&str>| super::PathRewrite {
            pattern: pattern.map(ToString::to_string),
            replace: replace.map(ToString::to_string),
            ..Default::default()
        };
        assert!(regex(Some("^/a/(\\d+)$"), None).compile().is_err());
        assert!(regex(Some("^/a/(\\d+"), Some("/b/$1"))
            .compile()
            .err()
            .unwrap()
            .starts_with("invalid `match`"));
        // Too large once compiled
        assert!(regex(Some("\\w{1000}{1000}"), Some("/")).compile().is_err());
        let mut combined = regex(Some("^/a$"), Some("/b"));
        combined.strip_prefix = Some("/a".to_string());
        assert!(combined.compile().is_err());
    }

    #[test]
    fn test_rewrites_with_regular_expressions() {
        let rewrite = super::PathRewrite {
            pattern: Some("^/users/(?P<id>\\d+)/profile$".to_string()),
            replace: Some("/profiles/${id}".to_string()),
            ..Default::default()
        }
        .compile()
        .unwrap();
        assert_eq!(
            rewrite.apply("/users/42/profile", Some("tab=posts")),
            "/profiles/42?tab=posts"
        );
        assert_eq!(rewrite.apply("/users/42", None), "/users/42");

        // Paths become absolute
        let relative = super::PathRewrite {
            pattern: Some("^/old/(.*)$".to_string()),
            replace: Some("$1".to_string()),
            ..Default::default()
        }
        .compile()
        .unwrap();
        assert_eq!(relative.apply("/old/page", None), "/page");
    }

    /// Cost of rewriting a path, per request: `cargo test --release -- --ignored rewrite_overhead
    /// --nocapture`
    #[test]
    #[ignore = "measures time"]
    fn rewrite_overhead() {
        let iterations = 1_000_000;
        let measure = |name: &str, rewrite: Option<super::CompiledRewrite>| {
            let started = std::time::Instant::now();
            for i in 0..iterations {
                let path = if i % 2 == 0 { "/users/42/profile" } else { "/users/42/posts" };
                let rewritten = match &rewrite {
                    Some(rewrite) => rewrite.apply(path, Some("tab=1")),
                    None => path.to_string(),
                };
                std::hint::black_box(rewritten);
            }
            let per = started.elapsed() / iterations;
            println!("{name}: {per:?} per request");
        };
        let compile = |rewrite: super::PathRewrite| Some(rewrite.compile().unwrap());

        measure("no rewrite", None);
        measure(
            "prefixes",
            compile(super::PathRewrite {
                strip_prefix: Some("/users".to_string()),
                add_prefix: Some("/v2".to_string()),
                ..Default::default()
            }),
        );
        measure(
            "regex",
            compile(super::PathRewrite {
                pattern: Some("^/users/(\\d+)/profile$".to_string()),
                replace: Some("/profiles/$1".to_string()),
                ..Default::default()
            }),
        );
        measure(
            "regex, named groups",
            compile(super::PathRewrite {
                pattern: Some("^/(?P<kind>[a-z]+)/(?P<id>\\d+)/(?P<rest>.*)$".to_string()),
                replace: Some("/api/${kind}/${rest}/${id}".to_string()),
                ..Default::default()
            }),
        );
    }
}
