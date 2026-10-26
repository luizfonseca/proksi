//! Which endpoint a request is for: a table of the API's paths, found with a path tree, and
//! the methods each one answers. Paths are matched as sent: unlike locations, they're not
//! normalized (`//api/v1/status` isn't an endpoint).

use std::{sync::LazyLock, time::Duration};

use http::{Method, StatusCode};
use path_tree::PathTree;

use super::{endpoint::Endpoint, Reply};

/// Stats windows, in seconds
const WINDOWS: std::ops::RangeInclusive<u64> = 1..=300;

/// A path of the API, before its method and parameters are read
#[derive(Debug, Clone, Copy)]
enum Path {
    Metrics,
    Stats,
    StatsStream,
    RequestsStream,
    Status,
    Server,
    Schema,
    Validate,
    Hosts,
    Tcp,
    Certificates,
    Routes,
    Route,
    RouteHistory,
    RouteVersion,
    RouteRollback,
    DeletedRoutes,
    Notifications,
    Notification,
    NotificationTest,
    Session,
}

/// Every path of the API
const PATHS: [(&str, Path); 21] = [
    ("/metrics", Path::Metrics),
    ("/api/v1/stats", Path::Stats),
    ("/api/v1/stats/stream", Path::StatsStream),
    ("/api/v1/requests/stream", Path::RequestsStream),
    ("/api/v1/status", Path::Status),
    ("/api/v1/server", Path::Server),
    ("/api/v1/schema", Path::Schema),
    ("/api/v1/validate", Path::Validate),
    ("/api/v1/hosts", Path::Hosts),
    ("/api/v1/tcp", Path::Tcp),
    ("/api/v1/certificates", Path::Certificates),
    ("/api/v1/routes", Path::Routes),
    ("/api/v1/routes/:host", Path::Route),
    ("/api/v1/routes/:host/history", Path::RouteHistory),
    ("/api/v1/routes/:host/history/:revision", Path::RouteVersion),
    ("/api/v1/routes/:host/rollback", Path::RouteRollback),
    ("/api/v1/deleted-routes", Path::DeletedRoutes),
    ("/api/v1/notifications", Path::Notifications),
    ("/api/v1/notifications/:name", Path::Notification),
    ("/api/v1/notifications/:name/test", Path::NotificationTest),
    ("/api/v1/session", Path::Session),
];

static ROUTER: LazyLock<PathTree<Path>> = LazyLock::new(|| {
    let mut tree = PathTree::new();
    for (pattern, path) in PATHS {
        let _ = tree.insert(pattern, path);
    }
    tree
});

/// The endpoint a request is for: `404` for a path that isn't the API's, `405` for a method
/// its path doesn't answer
pub fn parse<'a>(
    method: &Method,
    path: &'a str,
    query: Option<&'a str>,
) -> Result<Endpoint<'a>, Reply> {
    let (kind, found) = ROUTER.find(path).ok_or_else(not_found)?;
    // The parameters, in the order of the pattern: `:host` (or `:name`), then `:revision`
    let param = |index: usize| found.raws.get(index).copied().unwrap_or_default();
    let (host, revision) = (param(0), param(1));
    let endpoint = match (*kind, method.as_str()) {
        (Path::Metrics, "GET") => Endpoint::Metrics,
        (Path::Stats, "GET") => Endpoint::Stats(window(query, 60)?),
        (Path::StatsStream, "GET") => Endpoint::StatsStream(window(query, 10)?),
        (Path::RequestsStream, "GET") => Endpoint::RequestsStream(parameter(query, "host")),
        (Path::Status, "GET") => Endpoint::Status,
        (Path::Server, "GET") => Endpoint::Server,
        (Path::Schema, "GET") => Endpoint::Schema,
        (Path::Validate, "POST") => Endpoint::Validate,
        (Path::Hosts, "GET") => Endpoint::Hosts,
        (Path::Tcp, "GET") => Endpoint::Tcp,
        (Path::Certificates, "GET") => Endpoint::Certificates,
        (Path::Routes, "GET") => Endpoint::Routes,
        (Path::Route, "GET") => Endpoint::Route(host),
        (Path::Route, "PUT") => Endpoint::PutRoute(host),
        (Path::Route, "DELETE") => Endpoint::DeleteRoute(host),
        (Path::RouteHistory, "GET") => Endpoint::RouteHistory(host),
        (Path::RouteVersion, "GET") => {
            Endpoint::RouteVersion(host, revision.parse().map_err(|_| not_found())?)
        }
        (Path::RouteRollback, "POST") => Endpoint::RollbackRoute(host),
        (Path::DeletedRoutes, "GET") => Endpoint::DeletedRoutes,
        (Path::Notifications, "GET") => Endpoint::Notifications,
        (Path::Notification, "PUT") => Endpoint::PutNotification(host),
        (Path::Notification, "DELETE") => Endpoint::DeleteNotification(host),
        (Path::NotificationTest, "POST") => Endpoint::TestNotification(host),
        (Path::Session, "GET") => Endpoint::Session,
        (Path::Session, "POST") => Endpoint::SignIn,
        (Path::Session, "DELETE") => Endpoint::SignOut,
        _ => return Err(method_not_allowed()),
    };
    Ok(endpoint)
}

fn not_found() -> Reply {
    Reply::error(StatusCode::NOT_FOUND, "not found")
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
        super::parse(&method, path, query).map_err(|reply| reply.status)
    }

    #[test]
    fn test_paths_are_matched_as_sent() {
        for path in [
            "//api/v1/status",
            "/api/v1/status/",
            "/api//v1/status",
            "/API/v1/status",
        ] {
            assert_eq!(
                parse(Method::GET, path, None),
                Err(StatusCode::NOT_FOUND),
                "{path}"
            );
        }
    }

    #[test]
    fn test_route_history_endpoints() {
        assert_eq!(
            parse(Method::GET, "/api/v1/routes/a.test/history", None),
            Ok(Endpoint::RouteHistory("a.test"))
        );
        assert_eq!(
            parse(Method::GET, "/api/v1/routes/a.test/history/41", None),
            Ok(Endpoint::RouteVersion("a.test", 41))
        );
        assert_eq!(
            parse(Method::POST, "/api/v1/routes/a.test/rollback", None),
            Ok(Endpoint::RollbackRoute("a.test"))
        );
        assert_eq!(
            parse(Method::GET, "/api/v1/deleted-routes", None),
            Ok(Endpoint::DeletedRoutes)
        );
        let refused = [
            (
                Method::GET,
                "/api/v1/routes/a.test/history/x",
                StatusCode::NOT_FOUND,
            ),
            (
                Method::GET,
                "/api/v1/routes/a.test/history/41/x",
                StatusCode::NOT_FOUND,
            ),
            (
                Method::GET,
                "/api/v1/routes/a.test/rollback",
                StatusCode::METHOD_NOT_ALLOWED,
            ),
            (
                Method::POST,
                "/api/v1/routes/a.test/history",
                StatusCode::METHOD_NOT_ALLOWED,
            ),
            (
                Method::GET,
                "/api/v1/routes/a.test/other",
                StatusCode::NOT_FOUND,
            ),
        ];
        for (method, path, status) in refused {
            assert_eq!(parse(method, path, None), Err(status), "{path}");
        }
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
            parse(Method::PUT, "/api/v1/notifications/ops", None),
            Ok(Endpoint::PutNotification("ops"))
        );
        assert_eq!(
            parse(Method::DELETE, "/api/v1/notifications/ops", None),
            Ok(Endpoint::DeleteNotification("ops"))
        );
        assert_eq!(
            parse(Method::POST, "/api/v1/notifications/ops/test", None),
            Ok(Endpoint::TestNotification("ops"))
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
