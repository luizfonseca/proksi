//! The admin API (ADR 0004): metrics, live stats, and later status and routes

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

use crate::{
    config::admin::{Access, Token},
    metrics::live,
};

mod access;
pub mod routes;
pub mod status;
mod ui;

/// Stats windows, in seconds
const WINDOWS: std::ops::RangeInclusive<u64> = 1..=300;

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

impl Reply {
    fn json(status: StatusCode, body: &serde_json::Value) -> Self {
        Self {
            status,
            content_type: "application/json",
            body: body.to_string(),
            set_cookie: None,
        }
    }

    fn error(status: StatusCode, reason: &str) -> Self {
        Self::json(status, &serde_json::json!({ "error": reason }))
    }
}

/// What the admin API answers
#[derive(Debug, PartialEq)]
pub enum Answer {
    Reply(Reply),
    /// Server-sent events with the stats over this window, every second
    Stream(Duration),
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
        };
        let tokens = token
            .filter(|token| !token.is_empty())
            .map(|secret| {
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
        let Request {
            method,
            path,
            query,
            ..
        } = request;
        // The UI's files hold no data: its pages sign in to the API
        if !self.metrics_only && ui::serves(path) {
            return ui::answer(method, path, request.accept_encoding);
        }
        if !self.metrics_only && path == "/api/v1/session" {
            return Answer::Reply(self.session(request));
        }
        let Some(caller) = self.tokens.caller(request.authorization, request.cookie) else {
            return Answer::Reply(Reply::error(
                StatusCode::UNAUTHORIZED,
                "missing or invalid token",
            ));
        };
        let changes = method != Method::GET && method != Method::HEAD;
        // Other sites can't send the cookie (`SameSite=Strict`), and can't make changes with it
        if changes && caller.session && !access::same_origin(request.origin, request.host) {
            return Answer::Reply(Reply::error(
                StatusCode::FORBIDDEN,
                "changes must come from the admin's own pages (Origin)",
            ));
        }
        if changes && caller.access == Access::Read {
            return Answer::Reply(Reply::error(
                StatusCode::FORBIDDEN,
                "this token can only read (access = \"write\" to change hosts)",
            ));
        }

        if self.metrics_only && path != "/metrics" {
            return Answer::Reply(Reply::error(StatusCode::NOT_FOUND, "not found"));
        }
        if path == "/api/v1/routes" || path.starts_with("/api/v1/routes/") {
            return Answer::Reply(routes_reply(request).await);
        }

        let known = [
            "/metrics",
            "/api/v1/stats",
            "/api/v1/stats/stream",
            "/api/v1/status",
            "/api/v1/hosts",
            "/api/v1/certificates",
        ];
        if !known.contains(&path) {
            return Answer::Reply(Reply::error(StatusCode::NOT_FOUND, "not found"));
        }
        if method != Method::GET {
            return Answer::Reply(Reply::error(
                StatusCode::METHOD_NOT_ALLOWED,
                "method not allowed",
            ));
        }

        let metrics = ["/metrics", "/api/v1/stats", "/api/v1/stats/stream"];
        if metrics.contains(&path) && !crate::metrics::enabled() {
            return Answer::Reply(Reply::error(
                StatusCode::NOT_FOUND,
                "metrics are disabled (admin.metrics = false)",
            ));
        }

        match path {
            "/api/v1/stats" => match window(query, 60) {
                Ok(window) => Answer::Reply(Reply::json(StatusCode::OK, &stats(window))),
                Err(reply) => Answer::Reply(reply),
            },
            "/api/v1/stats/stream" => match window(query, 10) {
                Ok(window) => Answer::Stream(window),
                Err(reply) => Answer::Reply(reply),
            },
            "/api/v1/status" => {
                let store = crate::stores::global::get_store();
                let status = status::status(store.as_ref(), &self.instance).await;
                Answer::Reply(Reply::json(StatusCode::OK, &status))
            }
            "/api/v1/hosts" => Answer::Reply(Reply::json(StatusCode::OK, &status::hosts())),
            "/api/v1/certificates" => {
                let store = crate::stores::global::get_store();
                let certificates = status::certificates(store.as_ref()).await;
                Answer::Reply(Reply::json(StatusCode::OK, &certificates))
            }
            _ => Answer::Reply(Reply {
                status: StatusCode::OK,
                content_type: "text/plain; version=0.0.4",
                body: crate::metrics::gather(),
                set_cookie: None,
            }),
        }
    }

    /// `POST /api/v1/session` signs in with `{"token": "<secret>"}`, `GET` says who's signed
    /// in, `DELETE` signs out
    fn session(&self, request: Request<'_>) -> Reply {
        let unauthorized = || Reply::error(StatusCode::UNAUTHORIZED, "missing or invalid token");
        match *request.method {
            Method::GET => match self.tokens.caller(request.authorization, request.cookie) {
                Some(caller) => Reply::json(StatusCode::OK, &caller.json()),
                None => unauthorized(),
            },
            Method::POST => {
                if self.tokens.open() {
                    let caller = self.tokens.caller(None, None);
                    let body = caller.map(|caller| caller.json()).unwrap_or_default();
                    return Reply::json(StatusCode::OK, &body);
                }
                let secret = serde_json::from_slice::<serde_json::Value>(request.body)
                    .ok()
                    .and_then(|body| body.get("token")?.as_str().map(ToString::to_string));
                let Some((id, caller)) = secret.and_then(|secret| self.tokens.sign_in(&secret))
                else {
                    return unauthorized();
                };
                let mut reply = Reply::json(StatusCode::OK, &caller.json());
                reply.set_cookie = Some(access::set_cookie(Some(&id), request.host));
                reply
            }
            Method::DELETE => {
                self.tokens.sign_out(request.cookie);
                let mut reply = Reply::json(StatusCode::OK, &serde_json::json!({}));
                reply.set_cookie = Some(access::set_cookie(None, request.host));
                reply
            }
            _ => Reply::error(StatusCode::METHOD_NOT_ALLOWED, "method not allowed"),
        }
    }
}

/// `?window=<seconds>` (1 to 300), or `default`
fn window(query: Option<&str>, default: u64) -> Result<Duration, Reply> {
    let value = query
        .unwrap_or_default()
        .split('&')
        .find_map(|pair| pair.strip_prefix("window="));
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
        let content_type = value(header::CONTENT_TYPE);
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
                content_type: content_type.as_deref(),
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
                stream_stats(session, window, shutdown.clone()).await;
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
    pub content_type: Option<&'a str>,
    pub accept_encoding: Option<&'a str>,
    pub cookie: Option<&'a str>,
    pub origin: Option<&'a str>,
    pub host: Option<&'a str>,
    pub body: &'a [u8],
}

/// `GET /api/v1/routes`, `PUT` and `DELETE /api/v1/routes/<host>`
async fn routes_reply(request: Request<'_>) -> Reply {
    let store = crate::stores::global::get_store();
    let host = request
        .path
        .strip_prefix("/api/v1/routes/")
        .filter(|host| !host.is_empty() && !host.contains('/'));
    let result = match (request.method, host) {
        (&Method::GET, None) => routes::list(store.as_ref()).await,
        (&Method::PUT, Some(host)) => {
            routes::put(store.as_ref(), host, request.content_type, request.body).await
        }
        (&Method::DELETE, Some(host)) => routes::delete(store.as_ref(), host).await,
        (_, None) if request.path == "/api/v1/routes" => {
            return Reply::error(StatusCode::METHOD_NOT_ALLOWED, "method not allowed")
        }
        (_, Some(_)) => return Reply::error(StatusCode::METHOD_NOT_ALLOWED, "method not allowed"),
        _ => return Reply::error(StatusCode::NOT_FOUND, "not found"),
    };
    match result {
        Ok(body) => Reply::json(StatusCode::OK, &body),
        Err(routes::Refused::Invalid(reason)) => Reply::error(StatusCode::BAD_REQUEST, &reason),
        Err(routes::Refused::Unavailable(reason)) => {
            Reply::error(StatusCode::SERVICE_UNAVAILABLE, &reason)
        }
    }
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
        && header.insert_header(header::WWW_AUTHENTICATE, "Bearer").is_err()
    {
        return false;
    }
    if let Some(cookie) = reply.set_cookie {
        if header.insert_header(header::SET_COOKIE, cookie).is_err() {
            return false;
        }
    }
    if session.write_response_header(Box::new(header)).await.is_err() {
        return false;
    }
    session
        .write_response_body(Bytes::from(reply.body), true)
        .await
        .is_ok()
}

fn response_header(status: StatusCode, content_type: &str) -> Option<ResponseHeader> {
    let mut header = ResponseHeader::build(status, None).ok()?;
    header.insert_header(header::CONTENT_TYPE, content_type).ok()?;
    header.insert_header(header::CACHE_CONTROL, "no-store").ok()?;
    Some(header)
}

/// Sends the stats every second, until the client leaves or proksi shuts down
async fn stream_stats(mut session: ServerSession, window: Duration, mut shutdown: ShutdownWatch) {
    let Some(mut header) = response_header(StatusCode::OK, "text/event-stream") else {
        return;
    };
    if header
        .insert_header(header::TRANSFER_ENCODING, "chunked")
        .is_err()
        || session.write_response_header(Box::new(header)).await.is_err()
    {
        return;
    }

    let mut interval = tokio::time::interval(Duration::from_secs(1));
    loop {
        tokio::select! {
            _ = interval.tick() => {
                let event = format!("data: {}\n\n", stats(window));
                if session.write_response_body(Bytes::from(event), false).await.is_err() {
                    return;
                }
            }
            _ = shutdown.changed() => break,
        }
    }
    let _ = session.write_response_body(Bytes::new(), true).await;
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
            content_type: None,
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
            Answer::Stream(_) => panic!("a stream"),
            Answer::Ui(asset, _) => panic!("a file of the UI: {asset:?}"),
        }
    }

    #[tokio::test]
    async fn test_metrics() {
        crate::metrics::enable();
        let api = AdminApi::new(tokens(), instance());
        crate::metrics::LocationMetrics::unknown().record(404, Duration::from_millis(1));

        let reply = reply(api.handle(request(&Method::GET, "/metrics", None, Some("Bearer secret"))).await);

        assert_eq!(reply.status, StatusCode::OK);
        assert_eq!(reply.content_type, "text/plain; version=0.0.4");
        assert!(reply.body.contains("proksi_requests_total"), "{}", reply.body);
    }

    #[tokio::test]
    async fn test_metrics_only() {
        crate::metrics::enable();
        let api = AdminApi::metrics_only(None);

        assert_eq!(
            reply(api.handle(request(&Method::GET, "/metrics", None, None)).await).status,
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

        for authorization in [None, Some("Bearer wrong"), Some("secret"), Some("Basic secret")] {
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
                let reply = reply(api.handle(request(&Method::GET, path, None, authorization)).await);
                assert_eq!(reply.status, StatusCode::UNAUTHORIZED, "{authorization:?}");
                assert_eq!(reply.body, r#"{"error":"missing or invalid token"}"#);
            }
            // The UI's pages hold no data, and sign in to the API themselves
            let page = api.handle(request(&Method::GET, "/hosts", None, authorization)).await;
            assert!(matches!(page, Answer::Ui(..)), "{page:?}");
        }
    }

    #[tokio::test]
    async fn test_read_only_tokens() {
        let api = AdminApi::new(tokens(), instance());

        let read = reply(api.handle(request(&Method::GET, "/api/v1/routes", None, Some("Bearer read"))).await);
        assert_ne!(read.status, StatusCode::UNAUTHORIZED);
        for method in [Method::PUT, Method::DELETE] {
            let refused = reply(
                api.handle(request(&method, "/api/v1/routes/a.test", None, Some("Bearer read")))
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
        assert_eq!(signed_in.body, r#"{"access":"write","name":"ops"}"#);
        let set_cookie = signed_in.set_cookie.unwrap();
        assert!(set_cookie.ends_with("; Path=/; HttpOnly; SameSite=Strict"), "{set_cookie}");
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
        assert_eq!(reply(api.handle(who).await).body, r#"{"access":"write","name":"ops"}"#);
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
        assert!(reply(api.handle(sign_out).await).set_cookie.unwrap().contains("Max-Age=0"));
        let who = super::Request {
            path: "/api/v1/session",
            ..with_cookie(&Method::GET, None)
        };
        assert_eq!(reply(api.handle(who).await).status, StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn test_without_a_token() {
        crate::metrics::enable();
        // Only on a loopback address (checked when the configuration loads)
        let api = AdminApi::new(std::collections::BTreeMap::new(), instance());

        assert_eq!(
            reply(api.handle(request(&Method::GET, "/metrics", None, None)).await).status,
            StatusCode::OK
        );
    }

    #[tokio::test]
    async fn test_unknown_paths_and_methods() {
        let api = AdminApi::new(std::collections::BTreeMap::new(), instance());

        let not_found = reply(api.handle(request(&Method::GET, "/api/v1/nope", None, None)).await);
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
            api.handle(request(&Method::GET, "/api/v1/stats/stream", Some("window=5"), None)).await,
            Answer::Stream(Duration::from_secs(5))
        );
        assert_eq!(
            api.handle(request(&Method::GET, "/api/v1/stats/stream", None, None)).await,
            Answer::Stream(Duration::from_secs(10))
        );
    }
}
