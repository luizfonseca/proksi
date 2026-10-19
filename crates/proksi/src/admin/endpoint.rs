//! The admin API's endpoints, read from a request's method and path

use std::time::Duration;

use http::{Method, StatusCode};

use super::Reply;

/// Stats windows, in seconds
const WINDOWS: std::ops::RangeInclusive<u64> = 1..=300;

/// Paths answering other methods (`405` rather than `404`)
const PATHS: [&str; 13] = [
    "/api/v1/schema",
    "/api/v1/validate",
    "/metrics",
    "/api/v1/stats",
    "/api/v1/stats/stream",
    "/api/v1/requests/stream",
    "/api/v1/status",
    "/api/v1/server",
    "/api/v1/hosts",
    "/api/v1/tcp",
    "/api/v1/certificates",
    "/api/v1/routes",
    "/api/v1/session",
];

/// What a request asks for
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Endpoint<'a> {
    /// `GET /metrics`: Prometheus metrics
    Metrics,
    /// `GET /api/v1/stats?window=<seconds>`
    Stats(Duration),
    /// `GET /api/v1/stats/stream?window=<seconds>`: the stats every second
    StatsStream(Duration),
    /// `GET /api/v1/requests/stream?host=<host>`: the requests being served (of one host)
    RequestsStream(Option<&'a str>),
    /// `GET /api/v1/status`: version, store, route sources
    Status,
    /// `GET /api/v1/server`: the settings read at startup, without secrets
    Server,
    /// `GET /api/v1/schema`: what rules and blocks accept, for the UI's forms
    Schema,
    /// `POST /api/v1/validate`: checks a host (JSON) without writing it
    Validate,
    /// `GET /api/v1/hosts`: the hosts served, from every source
    Hosts,
    /// `GET /api/v1/tcp`: the `tcp` blocks
    Tcp,
    /// `GET /api/v1/certificates`
    Certificates,
    /// `GET /api/v1/routes`: the hosts in the store
    Routes,
    /// `GET /api/v1/routes/<host>`
    Route(&'a str),
    /// `PUT /api/v1/routes/<host>`, with the host as JSON
    PutRoute(&'a str),
    /// `DELETE /api/v1/routes/<host>`
    DeleteRoute(&'a str),
    /// `GET /api/v1/session`: who's signed in
    Session,
    /// `POST /api/v1/session`, with `{"token": "<secret>"}`
    SignIn,
    /// `DELETE /api/v1/session`
    SignOut,
}

impl<'a> Endpoint<'a> {
    pub fn parse(method: &Method, path: &'a str, query: Option<&'a str>) -> Result<Self, Reply> {
        use Endpoint::{
            Certificates, DeleteRoute, Hosts, Metrics, PutRoute, RequestsStream, Route, Routes, Schema, Validate,
            Server, Session, SignIn, SignOut, Stats, StatsStream, Status, Tcp,
        };

        if let Some(host) = path.strip_prefix("/api/v1/routes/") {
            if host.is_empty() || host.contains('/') {
                return Err(Reply::error(StatusCode::NOT_FOUND, "not found"));
            }
            return match *method {
                Method::GET => Ok(Route(host)),
                Method::PUT => Ok(PutRoute(host)),
                Method::DELETE => Ok(DeleteRoute(host)),
                _ => Err(method_not_allowed()),
            };
        }
        let endpoint = match (method.as_str(), path) {
            ("GET", "/metrics") => Metrics,
            ("GET", "/api/v1/stats") => Stats(window(query, 60)?),
            ("GET", "/api/v1/stats/stream") => StatsStream(window(query, 10)?),
            ("GET", "/api/v1/requests/stream") => RequestsStream(parameter(query, "host")),
            ("GET", "/api/v1/status") => Status,
            ("GET", "/api/v1/server") => Server,
            ("GET", "/api/v1/schema") => Schema,
            ("POST", "/api/v1/validate") => Validate,
            ("GET", "/api/v1/hosts") => Hosts,
            ("GET", "/api/v1/tcp") => Tcp,
            ("GET", "/api/v1/certificates") => Certificates,
            ("GET", "/api/v1/routes") => Routes,
            ("GET", "/api/v1/session") => Session,
            ("POST", "/api/v1/session") => SignIn,
            ("DELETE", "/api/v1/session") => SignOut,
            _ if PATHS.contains(&path) => return Err(method_not_allowed()),
            _ => return Err(Reply::error(StatusCode::NOT_FOUND, "not found")),
        };
        Ok(endpoint)
    }

    /// Whether it changes the configuration (a token with `access = "write"`)
    pub fn changes(self) -> bool {
        matches!(self, Endpoint::PutRoute(_) | Endpoint::DeleteRoute(_))
    }

    /// Whether it's answered without a token: signing in and out
    pub fn public(self) -> bool {
        matches!(self, Endpoint::SignIn | Endpoint::SignOut)
    }

    /// Whether it needs metrics to be recorded (`admin.metrics`)
    pub fn metrics(self) -> bool {
        matches!(
            self,
            Endpoint::Metrics | Endpoint::Stats(_) | Endpoint::StatsStream(_)
        )
    }
}

fn method_not_allowed() -> Reply {
    Reply::error(StatusCode::METHOD_NOT_ALLOWED, "method not allowed")
}

/// A query parameter's value, if not empty
fn parameter<'a>(query: Option<&'a str>, name: &str) -> Option<&'a str> {
    query
        .unwrap_or_default()
        .split('&')
        .find_map(|pair| pair.strip_prefix(name)?.strip_prefix('='))
        .filter(|value| !value.is_empty())
}

/// `?window=<seconds>` (1 to 300), or `default`
fn window(query: Option<&str>, default: u64) -> Result<Duration, Reply> {
    let value = parameter(query, "window");
    let Some(value) = value else {
        return Ok(Duration::from_secs(default));
    };
    match value.parse::<u64>() {
        Ok(seconds) if WINDOWS.contains(&seconds) => Ok(Duration::from_secs(seconds)),
        _ => Err(Reply::error(
            StatusCode::BAD_REQUEST,
            "window must be a number of seconds, from 1 to 300",
        )),
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use http::{Method, StatusCode};

    use super::Endpoint;

    fn parse<'a>(
        method: Method,
        path: &'a str,
        query: Option<&'a str>,
    ) -> Result<Endpoint<'a>, StatusCode> {
        Endpoint::parse(&method, path, query).map_err(|reply| reply.status)
    }

    #[test]
    fn test_endpoints() {
        assert_eq!(parse(Method::GET, "/metrics", None), Ok(Endpoint::Metrics));
        assert_eq!(
            parse(Method::GET, "/api/v1/requests/stream", None),
            Ok(Endpoint::RequestsStream(None))
        );
        assert_eq!(
            parse(Method::GET, "/api/v1/requests/stream", Some("host=a.test")),
            Ok(Endpoint::RequestsStream(Some("a.test")))
        );
        assert_eq!(
            parse(Method::GET, "/api/v1/server", None),
            Ok(Endpoint::Server)
        );
        assert_eq!(parse(Method::GET, "/api/v1/tcp", None), Ok(Endpoint::Tcp));
        assert_eq!(
            parse(Method::GET, "/api/v1/routes/a.test", None),
            Ok(Endpoint::Route("a.test"))
        );
        assert_eq!(
            parse(Method::PUT, "/api/v1/routes/a.test", None),
            Ok(Endpoint::PutRoute("a.test"))
        );
        assert_eq!(
            parse(Method::POST, "/api/v1/session", None),
            Ok(Endpoint::SignIn)
        );

        assert_eq!(
            parse(Method::GET, "/api/v1/nope", None),
            Err(StatusCode::NOT_FOUND)
        );
        assert_eq!(
            parse(Method::GET, "/api/v1/routes/", None),
            Err(StatusCode::NOT_FOUND)
        );
        assert_eq!(
            parse(Method::GET, "/api/v1/routes/a/b", None),
            Err(StatusCode::NOT_FOUND)
        );
        for path in [
            "/metrics",
            "/api/v1/stats",
            "/api/v1/routes",
            "/api/v1/routes/a.test",
        ] {
            assert_eq!(
                parse(Method::POST, path, None),
                Err(StatusCode::METHOD_NOT_ALLOWED),
                "{path}"
            );
        }
    }

    #[test]
    fn test_what_needs_write_access() {
        assert!(Endpoint::PutRoute("a.test").changes());
        assert!(Endpoint::DeleteRoute("a.test").changes());
        assert!(!Endpoint::Route("a.test").changes());
        assert!(!Endpoint::SignIn.changes());
    }

    #[test]
    fn test_stats_windows() {
        assert_eq!(
            parse(Method::GET, "/api/v1/stats", Some("window=30")),
            Ok(Endpoint::Stats(Duration::from_secs(30)))
        );
        assert_eq!(
            parse(Method::GET, "/api/v1/stats", None),
            Ok(Endpoint::Stats(Duration::from_secs(60)))
        );
        assert_eq!(
            parse(Method::GET, "/api/v1/stats/stream", None),
            Ok(Endpoint::StatsStream(Duration::from_secs(10)))
        );
        for invalid in ["window=0", "window=301", "window=abc"] {
            assert_eq!(
                parse(Method::GET, "/api/v1/stats", Some(invalid)),
                Err(StatusCode::BAD_REQUEST)
            );
        }
    }
}
