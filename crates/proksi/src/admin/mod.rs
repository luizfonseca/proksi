//! The admin API (ADR 0004, ADR 0008): metrics, live stats, hosts, the store's routes, and the UI

use std::{collections::BTreeMap, net::SocketAddr, sync::Arc, time::Duration};

use async_trait::async_trait;
use bytes::Bytes;
use http::{header, Method, StatusCode};
use pingora::{
    apps::{HttpPersistentSettings, HttpServerApp, ReusedHttpStream},
    http::ResponseHeader,
    protocols::http::ServerSession,
    server::ShutdownWatch,
    services::listening::Service,
};
use serde::{Deserialize, Serialize};

use crate::{
    config::admin::{Access, Token},
    metrics::live,
};

mod access;
mod endpoint;
pub mod routes;
pub mod server;
pub mod status;
mod stream;
mod ui;

use endpoint::Endpoint;

/// Largest request body accepted (a host)
const MAX_BODY: usize = 1024 * 1024;

/// A response of the admin API
#[derive(Debug, PartialEq)]
pub struct Reply {
    pub status: StatusCode,
    pub content_type: &'static str,
    pub body: String,
    /// `Set-Cookie`, signing in or out
    pub set_cookie: Option<String>,
}

/// The body of every error
#[derive(Serialize)]
struct Error<'a> {
    error: &'a str,
}

impl Reply {
    fn json(status: StatusCode, body: &impl Serialize) -> Self {
        let (status, body) = match serde_json::to_string(body) {
            Ok(body) => (status, body),
            Err(err) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!(r#"{{"error":"{err}"}}"#),
            ),
        };
        Self {
            status,
            content_type: "application/json",
            body,
            set_cookie: None,
        }
    }

    fn ok(body: &impl Serialize) -> Self {
        Self::json(StatusCode::OK, body)
    }

    fn error(status: StatusCode, reason: &str) -> Self {
        Self::json(status, &Error { error: reason })
    }

    fn unauthorized() -> Self {
        Self::error(StatusCode::UNAUTHORIZED, "missing or invalid token")
    }

    fn not_found() -> Self {
        Self::error(StatusCode::NOT_FOUND, "not found")
    }
}

/// `POST /api/v1/session`
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SignIn {
    /// A token's secret
    token: String,
}

/// What the admin API answers
#[derive(Debug, PartialEq)]
pub enum Answer {
    Reply(Reply),
    /// Server-sent events with the stats over this window, every second
    Stream(Duration),
    /// Server-sent events with the requests being served
    Requests,
    /// A file of the UI
    Ui(&'static proksi_admin_ui::Asset, ui::Encoding),
}

pub struct AdminApi {
    tokens: access::Tokens,
    instance: status::Instance,
    /// Serves `/metrics` only (`admin.metrics_listen`)
    metrics_only: bool,
}

impl AdminApi {
    pub fn new(tokens: BTreeMap<String, Token>, instance: status::Instance) -> Self {
        Self {
            tokens: access::Tokens::new(tokens),
            instance,
            metrics_only: false,
        }
    }

    /// The API with `/metrics` only, for scrapers that mustn't change routes
    pub fn metrics_only(token: Option<String>) -> Self {
        let instance = status::Instance {
            started: std::time::Instant::now(),
            store_type: String::new(),
            listeners: serde_json::Value::Null,
            server: None,
        };
        let tokens = token.filter(|token| !token.is_empty()).map(|secret| {
            let access = Access::Read;
            ("metrics".to_string(), Token { secret, access })
        });
        Self {
            tokens: access::Tokens::new(tokens.into_iter().collect()),
            instance,
            metrics_only: true,
        }
    }

    /// Answers a request
    pub async fn handle(&self, request: Request<'_>) -> Answer {
        // The UI's files hold no data: its pages sign in to the API
        if !self.metrics_only && ui::serves(request.path) {
            return ui::answer(request.method, request.path, request.accept_encoding);
        }
        let endpoint = Endpoint::parse(request.method, request.path, request.query);
        if let Ok(endpoint) = endpoint.as_ref().copied() {
            if endpoint.public() && !self.metrics_only {
                return Answer::Reply(self.sign_in_or_out(endpoint, request));
            }
        }
        let Some(caller) = self.tokens.caller(request.authorization, request.cookie) else {
            return Answer::Reply(Reply::unauthorized());
        };
        let endpoint = match endpoint {
            Ok(endpoint) if !self.metrics_only || endpoint == Endpoint::Metrics => endpoint,
            Ok(_) => return Answer::Reply(Reply::not_found()),
            Err(reply) => return Answer::Reply(reply),
        };
        if endpoint.changes() {
            // Other sites can't send the cookie (`SameSite=Strict`), nor make changes with it
            if caller.session && !access::same_origin(request.origin, request.host) {
                return Answer::Reply(Reply::error(
                    StatusCode::FORBIDDEN,
                    "changes must come from the admin's own pages (Origin)",
                ));
            }
            if caller.access == Access::Read {
                return Answer::Reply(Reply::error(
                    StatusCode::FORBIDDEN,
                    "this token can only read (access = \"write\" to change hosts)",
                ));
            }
        }
        if endpoint.metrics() && !crate::metrics::enabled() {
            return Answer::Reply(Reply::error(
                StatusCode::NOT_FOUND,
                "metrics are disabled (admin.metrics = false)",
            ));
        }

        let store = crate::stores::global::get_store();
        let store = store.as_ref();
        let reply = match endpoint {
            Endpoint::Metrics => Reply {
                status: StatusCode::OK,
                content_type: "text/plain; version=0.0.4",
                body: crate::metrics::gather(),
                set_cookie: None,
            },
            Endpoint::Stats(window) => Reply::ok(&stats(window)),
            Endpoint::StatsStream(window) => return Answer::Stream(window),
            Endpoint::RequestsStream => return Answer::Requests,
            Endpoint::Status => Reply::ok(&status::status(store, &self.instance).await),
            Endpoint::Server => match &self.instance.server {
                Some(server) => Reply::ok(server),
                None => Reply::not_found(),
            },
            Endpoint::Hosts => Reply::ok(&status::hosts()),
            Endpoint::Certificates => Reply::ok(&status::certificates(store).await),
            Endpoint::Routes => routes_reply(routes::list(store).await),
            Endpoint::Route(host) => routes_reply(routes::get(store, host).await),
            Endpoint::PutRoute(host) => routes_reply(routes::put(store, host, request.body).await),
            Endpoint::DeleteRoute(host) => routes_reply(routes::delete(store, host).await),
            Endpoint::Session => Reply::ok(&caller),
            Endpoint::SignIn | Endpoint::SignOut => Reply::not_found(),
        };
        Answer::Reply(reply)
    }

    /// `POST /api/v1/session` exchanges a token for a session cookie; `DELETE` ends it
    fn sign_in_or_out(&self, endpoint: Endpoint<'_>, request: Request<'_>) -> Reply {
        if endpoint == Endpoint::SignOut {
            self.tokens.sign_out(request.cookie);
            let mut reply = Reply::ok(&serde_json::json!({}));
            reply.set_cookie = Some(access::set_cookie(None, request.host));
            return reply;
        }
        // Without tokens, there's no one to sign in as
        if let Some(caller) = self
            .tokens
            .caller(None, None)
            .filter(|_| self.tokens.open())
        {
            return Reply::ok(&caller);
        }
        let Ok(SignIn { token }) = serde_json::from_slice::<SignIn>(request.body) else {
            return Reply::error(StatusCode::BAD_REQUEST, r#"expected {"token": "<secret>"}"#);
        };
        let Some((id, caller)) = self.tokens.sign_in(&token) else {
            return Reply::unauthorized();
        };
        let mut reply = Reply::ok(&caller);
        reply.set_cookie = Some(access::set_cookie(Some(&id), request.host));
        reply
    }
}

/// A reply of `/api/v1/routes`
fn routes_reply(result: Result<impl Serialize, routes::Refused>) -> Reply {
    match result {
        Ok(body) => Reply::ok(&body),
        Err(routes::Refused::Invalid(reason)) => Reply::error(StatusCode::BAD_REQUEST, &reason),
        Err(routes::Refused::NotFound(reason)) => Reply::error(StatusCode::NOT_FOUND, &reason),
        Err(routes::Refused::Unavailable(reason)) => {
            Reply::error(StatusCode::SERVICE_UNAVAILABLE, &reason)
        }
    }
}

/// The stats over `window` (since the oldest sample if proksi started more recently)
fn stats(window: Duration) -> serde_json::Value {
    if let Some((earlier, latest)) = live::window(window) {
        return live::stats(&earlier, &latest);
    }
    let now = live::sample();
    live::stats(&now, &now)
}

#[async_trait]
impl HttpServerApp for AdminApi {
    async fn process_new_http(
        self: &Arc<Self>,
        mut session: ServerSession,
        shutdown: &ShutdownWatch,
    ) -> Option<ReusedHttpStream> {
        if !matches!(session.read_request().await, Ok(true)) {
            return None;
        }
        let request = session.req_header();
        let (method, uri) = (request.method.clone(), request.uri.clone());
        let value = |name: header::HeaderName| {
            request
                .headers
                .get(name)
                .and_then(|value| value.to_str().ok())
                .map(ToString::to_string)
        };
        let authorization = value(header::AUTHORIZATION);
        let accept_encoding = value(header::ACCEPT_ENCODING);
        let cookie = value(header::COOKIE);
        let origin = value(header::ORIGIN);
        let host = value(header::HOST);

        let body = if method == Method::PUT || method == Method::POST {
            match read_body(&mut session).await {
                Ok(body) => body,
                Err(reply) => {
                    write_reply(&mut session, reply).await;
                    return None;
                }
            }
        } else {
            vec![]
        };

        let answer = self
            .handle(Request {
                method: &method,
                path: uri.path(),
                query: uri.query(),
                authorization: authorization.as_deref(),
                accept_encoding: accept_encoding.as_deref(),
                cookie: cookie.as_deref(),
                origin: origin.as_deref(),
                host: host.as_deref(),
                body: &body,
            })
            .await;

        let written = match answer {
            Answer::Reply(reply) => write_reply(&mut session, reply).await,
            Answer::Ui(asset, encoding) => {
                ui::write(&mut session, asset, encoding, method == Method::HEAD).await
            }
            Answer::Stream(window) => {
                stream::stats(session, window, shutdown.clone()).await;
                return None;
            }
            Answer::Requests => {
                stream::requests(session, shutdown.clone()).await;
                return None;
            }
        };
        if !written {
            return None;
        }
        let persistent_settings = HttpPersistentSettings::for_session(&session);
        session
            .finish()
            .await
            .ok()
            .flatten()
            .map(|stream| ReusedHttpStream::from_reusable_stream(stream, persistent_settings))
    }
}

/// A request to the admin API
#[derive(Clone, Copy)]
pub struct Request<'a> {
    pub method: &'a Method,
    pub path: &'a str,
    pub query: Option<&'a str>,
    pub authorization: Option<&'a str>,
    pub accept_encoding: Option<&'a str>,
    pub cookie: Option<&'a str>,
    pub origin: Option<&'a str>,
    pub host: Option<&'a str>,
    pub body: &'a [u8],
}

/// The request body, up to `MAX_BODY`
async fn read_body(session: &mut ServerSession) -> Result<Vec<u8>, Reply> {
    let mut body = vec![];
    loop {
        match session.read_request_body().await {
            Ok(Some(chunk)) => {
                if body.len() + chunk.len() > MAX_BODY {
                    return Err(Reply::error(
                        StatusCode::PAYLOAD_TOO_LARGE,
                        "the body is larger than 1 MiB",
                    ));
                }
                body.extend_from_slice(&chunk);
            }
            Ok(None) => return Ok(body),
            Err(err) => {
                return Err(Reply::error(
                    StatusCode::BAD_REQUEST,
                    &format!("failed to read the body: {err}"),
                ))
            }
        }
    }
}

/// Writes a whole reply; returns whether it was written
async fn write_reply(session: &mut ServerSession, reply: Reply) -> bool {
    let Some(mut header) = response_header(reply.status, reply.content_type) else {
        return false;
    };
    if header
        .insert_header(header::CONTENT_LENGTH, reply.body.len())
        .is_err()
    {
        return false;
    }
    if reply.status == StatusCode::UNAUTHORIZED
        && header
            .insert_header(header::WWW_AUTHENTICATE, "Bearer")
            .is_err()
    {
        return false;
    }
    if let Some(cookie) = reply.set_cookie {
        if header.insert_header(header::SET_COOKIE, cookie).is_err() {
            return false;
        }
    }
    if session
        .write_response_header(Box::new(header))
        .await
        .is_err()
    {
        return false;
    }
    session
        .write_response_body(Bytes::from(reply.body), true)
        .await
        .is_ok()
}

fn response_header(status: StatusCode, content_type: &str) -> Option<ResponseHeader> {
    let mut header = ResponseHeader::build(status, None).ok()?;
    header
        .insert_header(header::CONTENT_TYPE, content_type)
        .ok()?;
    header
        .insert_header(header::CACHE_CONTROL, "no-store")
        .ok()?;
    Some(header)
}

/// The admin API's service, on `address`
/// The metrics-only listener (`admin.metrics_listen`)
pub fn metrics_service(address: SocketAddr, token: Option<String>) -> Service<AdminApi> {
    let mut service = Service::new("metrics".to_string(), AdminApi::metrics_only(token));
    service.add_tcp(&address.to_string());
    service.threads = Some(1);
    service
}

pub fn admin_service(
    address: SocketAddr,
    tokens: BTreeMap<String, Token>,
    instance: status::Instance,
) -> Service<AdminApi> {
    let mut service = Service::new("admin api".to_string(), AdminApi::new(tokens, instance));
    service.add_tcp(&address.to_string());
    service.threads = Some(1);
    service
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use http::{Method, StatusCode};

    use super::{AdminApi, Answer, Reply};
    use crate::config::admin::{Access, Token};

    fn instance() -> super::status::Instance {
        super::status::Instance {
            started: std::time::Instant::now(),
            store_type: "memory".to_string(),
            listeners: serde_json::json!({}),
            server: None,
        }
    }

    /// `ops` can write (`Bearer secret`), `grafana` can only read (`Bearer read`)
    fn tokens() -> std::collections::BTreeMap<String, Token> {
        std::collections::BTreeMap::from([
            (
                "ops".to_string(),
                Token {
                    secret: "secret".to_string(),
                    access: Access::Write,
                },
            ),
            (
                "grafana".to_string(),
                Token {
                    secret: "read".to_string(),
                    access: Access::Read,
                },
            ),
        ])
    }

    fn request<'a>(
        method: &'a Method,
        path: &'a str,
        query: Option<&'a str>,
        authorization: Option<&'a str>,
    ) -> super::Request<'a> {
        super::Request {
            method,
            path,
            query,
            authorization,
            accept_encoding: None,
            cookie: None,
            origin: None,
            host: None,
            body: &[],
        }
    }

    fn reply(answer: Answer) -> Reply {
        match answer {
            Answer::Reply(reply) => reply,
            Answer::Stream(_) | Answer::Requests => panic!("a stream"),
            Answer::Ui(asset, _) => panic!("a file of the UI: {asset:?}"),
        }
    }

    #[tokio::test]
    async fn test_metrics() {
        crate::metrics::enable();
        let api = AdminApi::new(tokens(), instance());
        crate::metrics::LocationMetrics::unknown().record(404, Duration::from_millis(1));

        let reply = reply(
            api.handle(request(
                &Method::GET,
                "/metrics",
                None,
                Some("Bearer secret"),
            ))
            .await,
        );

        assert_eq!(reply.status, StatusCode::OK);
        assert_eq!(reply.content_type, "text/plain; version=0.0.4");
        assert!(
            reply.body.contains("proksi_requests_total"),
            "{}",
            reply.body
        );
    }

    #[tokio::test]
    async fn test_metrics_only() {
        crate::metrics::enable();
        let api = AdminApi::metrics_only(None);

        assert_eq!(
            reply(
                api.handle(request(&Method::GET, "/metrics", None, None))
                    .await
            )
            .status,
            StatusCode::OK
        );
        for (method, path) in [
            (Method::GET, "/api/v1/status"),
            (Method::GET, "/api/v1/stats"),
            (Method::GET, "/api/v1/routes"),
            (Method::PUT, "/api/v1/routes/a.test"),
            (Method::DELETE, "/api/v1/routes/a.test"),
        ] {
            let reply = reply(api.handle(request(&method, path, None, None)).await);
            assert_eq!(reply.status, StatusCode::NOT_FOUND, "{method} {path}");
        }

        // With its own token
        let api = AdminApi::metrics_only(Some("scraper".to_string()));
        for (authorization, status) in [
            (None, StatusCode::UNAUTHORIZED),
            (Some("Bearer scraper"), StatusCode::OK),
        ] {
            let answer = api
                .handle(request(&Method::GET, "/metrics", None, authorization))
                .await;
            assert_eq!(reply(answer).status, status, "{authorization:?}");
        }
    }

    #[tokio::test]
    async fn test_token() {
        let api = AdminApi::new(tokens(), instance());

        for authorization in [
            None,
            Some("Bearer wrong"),
            Some("secret"),
            Some("Basic secret"),
        ] {
            for path in [
                "/metrics",
                "/api/v1/stats",
                "/api/v1/stats/stream",
                "/api/v1/status",
                "/api/v1/hosts",
                "/api/v1/certificates",
                "/api/v1/routes",
                "/api/v1/routes/a.test",
                "/api/v1/nope",
            ] {
                let reply = reply(
                    api.handle(request(&Method::GET, path, None, authorization))
                        .await,
                );
                assert_eq!(reply.status, StatusCode::UNAUTHORIZED, "{authorization:?}");
                assert_eq!(reply.body, r#"{"error":"missing or invalid token"}"#);
            }
            // The UI's pages hold no data, and sign in to the API themselves
            let page = api
                .handle(request(&Method::GET, "/hosts", None, authorization))
                .await;
            assert!(matches!(page, Answer::Ui(..)), "{page:?}");
        }
    }

    #[tokio::test]
    async fn test_read_only_tokens() {
        let api = AdminApi::new(tokens(), instance());

        let read = reply(
            api.handle(request(
                &Method::GET,
                "/api/v1/routes",
                None,
                Some("Bearer read"),
            ))
            .await,
        );
        assert_ne!(read.status, StatusCode::UNAUTHORIZED);
        for method in [Method::PUT, Method::DELETE] {
            let refused = reply(
                api.handle(request(
                    &method,
                    "/api/v1/routes/a.test",
                    None,
                    Some("Bearer read"),
                ))
                .await,
            );
            assert_eq!(refused.status, StatusCode::FORBIDDEN, "{method}");
        }
    }

    #[tokio::test]
    async fn test_sessions() {
        let api = AdminApi::new(tokens(), instance());
        let sign_in = |body: &'static [u8]| super::Request {
            body,
            host: Some("127.0.0.1:7555"),
            ..request(&Method::POST, "/api/v1/session", None, None)
        };

        let refused = reply(api.handle(sign_in(br#"{"token":"wrong"}"#)).await);
        assert_eq!(refused.status, StatusCode::UNAUTHORIZED);
        assert_eq!(refused.set_cookie, None);

        let signed_in = reply(api.handle(sign_in(br#"{"token":"secret"}"#)).await);
        assert_eq!(signed_in.status, StatusCode::OK);
        assert_eq!(signed_in.body, r#"{"name":"ops","access":"write"}"#);
        let set_cookie = signed_in.set_cookie.unwrap();
        assert!(
            set_cookie.ends_with("; Path=/; HttpOnly; SameSite=Strict"),
            "{set_cookie}"
        );
        let cookie = set_cookie.split(';').next().unwrap();

        let with_cookie = |method, origin| super::Request {
            cookie: Some(cookie),
            origin,
            host: Some("127.0.0.1:7555"),
            ..request(method, "/api/v1/routes/a.test", None, None)
        };
        let who = super::Request {
            path: "/api/v1/session",
            ..with_cookie(&Method::GET, None)
        };
        assert_eq!(
            reply(api.handle(who).await).body,
            r#"{"name":"ops","access":"write"}"#
        );
        // Changes with the cookie must come from the admin's own pages
        for origin in [None, Some("http://evil.test")] {
            let refused = reply(api.handle(with_cookie(&Method::DELETE, origin)).await);
            assert_eq!(refused.status, StatusCode::FORBIDDEN, "{origin:?}");
        }
        let allowed = reply(
            api.handle(with_cookie(&Method::DELETE, Some("http://127.0.0.1:7555")))
                .await,
        );
        assert_ne!(allowed.status, StatusCode::FORBIDDEN);

        let sign_out = super::Request {
            path: "/api/v1/session",
            ..with_cookie(&Method::DELETE, Some("http://127.0.0.1:7555"))
        };
        assert!(reply(api.handle(sign_out).await)
            .set_cookie
            .unwrap()
            .contains("Max-Age=0"));
        let who = super::Request {
            path: "/api/v1/session",
            ..with_cookie(&Method::GET, None)
        };
        assert_eq!(
            reply(api.handle(who).await).status,
            StatusCode::UNAUTHORIZED
        );
    }

    #[tokio::test]
    async fn test_without_a_token() {
        crate::metrics::enable();
        // Only on a loopback address (checked when the configuration loads)
        let api = AdminApi::new(std::collections::BTreeMap::new(), instance());

        assert_eq!(
            reply(
                api.handle(request(&Method::GET, "/metrics", None, None))
                    .await
            )
            .status,
            StatusCode::OK
        );
    }

    #[tokio::test]
    async fn test_unknown_paths_and_methods() {
        let api = AdminApi::new(std::collections::BTreeMap::new(), instance());

        let not_found = reply(
            api.handle(request(&Method::GET, "/api/v1/nope", None, None))
                .await,
        );
        assert_eq!(not_found.status, StatusCode::NOT_FOUND);
        assert_eq!(not_found.body, r#"{"error":"not found"}"#);
        for path in ["/metrics", "/api/v1/stats"] {
            assert_eq!(
                reply(api.handle(request(&Method::POST, path, None, None)).await).status,
                StatusCode::METHOD_NOT_ALLOWED
            );
        }
    }

    #[tokio::test]
    async fn test_stats_windows() {
        crate::metrics::enable();
        let api = AdminApi::new(std::collections::BTreeMap::new(), instance());
        let stats = |query| api.handle(request(&Method::GET, "/api/v1/stats", query, None));

        let ok = reply(stats(Some("window=30")).await);
        assert_eq!(ok.status, StatusCode::OK);
        assert_eq!(ok.content_type, "application/json");
        assert!(ok.body.contains("\"hosts\""), "{}", ok.body);
        assert_eq!(reply(stats(None).await).status, StatusCode::OK);
        for invalid in ["window=0", "window=301", "window=abc"] {
            assert_eq!(
                reply(stats(Some(invalid)).await).status,
                StatusCode::BAD_REQUEST
            );
        }

        assert_eq!(
            api.handle(request(
                &Method::GET,
                "/api/v1/stats/stream",
                Some("window=5"),
                None
            ))
            .await,
            Answer::Stream(Duration::from_secs(5))
        );
        assert_eq!(
            api.handle(request(&Method::GET, "/api/v1/stats/stream", None, None))
                .await,
            Answer::Stream(Duration::from_secs(10))
        );
    }
}
