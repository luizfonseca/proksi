//! The admin API's endpoints, read from a request's method and path

use std::time::Duration;

use http::Method;

use super::Reply;

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
    /// `GET /api/v1/routes/<host>/history`: its changes, newest first
    RouteHistory(&'a str),
    /// `GET /api/v1/routes/<host>/history/<revision>`: the version saved at that revision
    RouteVersion(&'a str, u64),
    /// `POST /api/v1/routes/<host>/rollback`, with `{"revision": <version>}`
    RollbackRoute(&'a str),
    /// `GET /api/v1/deleted-routes`: deleted hosts that still have versions
    DeletedRoutes,
    /// `GET /api/v1/session`: who's signed in
    Session,
    /// `POST /api/v1/session`, with `{"token": "<secret>"}`
    SignIn,
    /// `DELETE /api/v1/session`
    SignOut,
}

impl<'a> Endpoint<'a> {
    /// The endpoint a request is for (see [`super::router`])
    pub fn parse(method: &Method, path: &'a str, query: Option<&'a str>) -> Result<Self, Reply> {
        super::router::parse(method, path, query)
    }

    /// Whether it changes the configuration (a token with `access = "write"`)
    pub fn changes(self) -> bool {
        matches!(
            self,
            Endpoint::PutRoute(_) | Endpoint::DeleteRoute(_) | Endpoint::RollbackRoute(_)
        )
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

#[cfg(test)]
mod tests {
    use super::Endpoint;

    #[test]
    fn test_what_needs_write_access() {
        assert!(Endpoint::PutRoute("a.test").changes());
        assert!(Endpoint::DeleteRoute("a.test").changes());
        assert!(Endpoint::RollbackRoute("a.test").changes());
        assert!(!Endpoint::RouteHistory("a.test").changes());
        assert!(!Endpoint::Route("a.test").changes());
        assert!(!Endpoint::SignIn.changes());
    }
}
