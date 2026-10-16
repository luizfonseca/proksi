//! The admin API (ADR 0004): metrics, live stats, and later status and routes

use std::{net::SocketAddr, sync::Arc, time::Duration};

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

use crate::metrics::live;

pub mod status;

/// Stats windows, in seconds
const WINDOWS: std::ops::RangeInclusive<u64> = 1..=300;

/// A response of the admin API
#[derive(Debug, PartialEq)]
pub struct Reply {
    pub status: StatusCode,
    pub content_type: &'static str,
    pub body: String,
}

impl Reply {
    fn json(status: StatusCode, body: &serde_json::Value) -> Self {
        Self {
            status,
            content_type: "application/json",
            body: body.to_string(),
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
}

pub struct AdminApi {
    token: Option<String>,
    instance: status::Instance,
}

impl AdminApi {
    pub fn new(token: Option<String>, instance: status::Instance) -> Self {
        Self { token, instance }
    }

    /// Answers a request, given its `Authorization` header
    pub async fn handle(
        &self,
        method: &Method,
        path: &str,
        query: Option<&str>,
        authorization: Option<&str>,
    ) -> Answer {
        if !self.authorized(authorization) {
            return Answer::Reply(Reply::error(
                StatusCode::UNAUTHORIZED,
                "missing or invalid token",
            ));
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
            }),
        }
    }

    /// `Authorization: Bearer <token>`, compared in constant time
    fn authorized(&self, authorization: Option<&str>) -> bool {
        let Some(token) = &self.token else {
            return true;
        };
        authorization
            .and_then(|value| value.strip_prefix("Bearer "))
            .is_some_and(|given| {
                given.len() == token.len()
                    && openssl::memcmp::eq(given.as_bytes(), token.as_bytes())
            })
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
        let authorization = request
            .headers
            .get(header::AUTHORIZATION)
            .and_then(|value| value.to_str().ok());
        let (method, uri) = (request.method.clone(), request.uri.clone());
        let authorization = authorization.map(ToString::to_string);
        let answer = self
            .handle(&method, uri.path(), uri.query(), authorization.as_deref())
            .await;

        match answer {
            Answer::Reply(reply) => {
                let mut header = response_header(reply.status, reply.content_type)?;
                header
                    .insert_header(header::CONTENT_LENGTH, reply.body.len())
                    .ok()?;
                if reply.status == StatusCode::UNAUTHORIZED {
                    header.insert_header(header::WWW_AUTHENTICATE, "Bearer").ok()?;
                }
                session.write_response_header(Box::new(header)).await.ok()?;
                session
                    .write_response_body(Bytes::from(reply.body), true)
                    .await
                    .ok()?;
                let persistent_settings = HttpPersistentSettings::for_session(&session);
                session
                    .finish()
                    .await
                    .ok()
                    .flatten()
                    .map(|stream| ReusedHttpStream::from_reusable_stream(stream, persistent_settings))
            }
            Answer::Stream(window) => {
                stream_stats(session, window, shutdown.clone()).await;
                None
            }
        }
    }
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
pub fn admin_service(
    address: SocketAddr,
    token: Option<String>,
    instance: status::Instance,
) -> Service<AdminApi> {
    let mut service = Service::new("admin api".to_string(), AdminApi::new(token, instance));
    service.add_tcp(&address.to_string());
    service.threads = Some(1);
    service
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use http::{Method, StatusCode};

    use super::{AdminApi, Answer, Reply};

    fn instance() -> super::status::Instance {
        super::status::Instance {
            started: std::time::Instant::now(),
            store_type: "memory".to_string(),
            listeners: serde_json::json!({}),
        }
    }

    fn reply(answer: Answer) -> Reply {
        match answer {
            Answer::Reply(reply) => reply,
            Answer::Stream(_) => panic!("a stream"),
        }
    }

    #[tokio::test]
    async fn test_metrics() {
        let api = AdminApi::new(Some("secret".to_string()), instance());
        crate::metrics::LocationMetrics::unknown().record(404, Duration::from_millis(1));

        let reply = reply(api.handle(&Method::GET, "/metrics", None, Some("Bearer secret")).await);

        assert_eq!(reply.status, StatusCode::OK);
        assert_eq!(reply.content_type, "text/plain; version=0.0.4");
        assert!(reply.body.contains("proksi_requests_total"), "{}", reply.body);
    }

    #[tokio::test]
    async fn test_token() {
        let api = AdminApi::new(Some("secret".to_string()), instance());

        for authorization in [None, Some("Bearer wrong"), Some("secret"), Some("Basic secret")] {
            for path in [
                "/metrics",
                "/api/v1/stats",
                "/api/v1/stats/stream",
                "/api/v1/status",
                "/api/v1/hosts",
                "/api/v1/certificates",
                "/nope",
            ] {
                let reply = reply(api.handle(&Method::GET, path, None, authorization).await);
                assert_eq!(reply.status, StatusCode::UNAUTHORIZED, "{authorization:?}");
                assert_eq!(reply.body, r#"{"error":"missing or invalid token"}"#);
            }
        }
    }

    #[tokio::test]
    async fn test_without_a_token() {
        // Only on a loopback address (checked when the configuration loads)
        let api = AdminApi::new(None, instance());

        assert_eq!(
            reply(api.handle(&Method::GET, "/metrics", None, None).await).status,
            StatusCode::OK
        );
    }

    #[tokio::test]
    async fn test_unknown_paths_and_methods() {
        let api = AdminApi::new(None, instance());

        let not_found = reply(api.handle(&Method::GET, "/nope", None, None).await);
        assert_eq!(not_found.status, StatusCode::NOT_FOUND);
        assert_eq!(not_found.body, r#"{"error":"not found"}"#);
        for path in ["/metrics", "/api/v1/stats"] {
            assert_eq!(
                reply(api.handle(&Method::POST, path, None, None).await).status,
                StatusCode::METHOD_NOT_ALLOWED
            );
        }
    }

    #[tokio::test]
    async fn test_stats_windows() {
        let api = AdminApi::new(None, instance());
        let stats = |query| api.handle(&Method::GET, "/api/v1/stats", query, None);

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
            api.handle(&Method::GET, "/api/v1/stats/stream", Some("window=5"), None).await,
            Answer::Stream(Duration::from_secs(5))
        );
        assert_eq!(
            api.handle(&Method::GET, "/api/v1/stats/stream", None, None).await,
            Answer::Stream(Duration::from_secs(10))
        );
    }
}
