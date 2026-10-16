//! The admin API (ADR 0004): metrics, and later live stats, status and routes

use std::net::SocketAddr;

use async_trait::async_trait;
use http::{header, Method, Response, StatusCode};
use pingora::{
    apps::http_app::{HttpServer, ServeHttp},
    protocols::http::ServerSession,
    services::listening::Service,
};

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

pub struct AdminApi {
    token: Option<String>,
}

impl AdminApi {
    pub fn new(token: Option<String>) -> Self {
        Self { token }
    }

    /// Answers a request, given its `Authorization` header
    pub fn handle(&self, method: &Method, path: &str, authorization: Option<&str>) -> Reply {
        if !self.authorized(authorization) {
            return Reply::error(StatusCode::UNAUTHORIZED, "missing or invalid token");
        }

        match path {
            "/metrics" if method == Method::GET => Reply {
                status: StatusCode::OK,
                content_type: "text/plain; version=0.0.4",
                body: crate::metrics::gather(),
            },
            "/metrics" => Reply::error(StatusCode::METHOD_NOT_ALLOWED, "method not allowed"),
            _ => Reply::error(StatusCode::NOT_FOUND, "not found"),
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

#[async_trait]
impl ServeHttp for AdminApi {
    async fn response(&self, session: &mut ServerSession) -> Response<Vec<u8>> {
        let request = session.req_header();
        let authorization = request
            .headers
            .get(header::AUTHORIZATION)
            .and_then(|value| value.to_str().ok());
        let reply = self.handle(&request.method, request.uri.path(), authorization);

        let mut response = Response::builder()
            .status(reply.status)
            .header(header::CONTENT_TYPE, reply.content_type)
            .header(header::CONTENT_LENGTH, reply.body.len())
            .header(header::CACHE_CONTROL, "no-store");
        if reply.status == StatusCode::UNAUTHORIZED {
            response = response.header(header::WWW_AUTHENTICATE, "Bearer");
        }
        response
            .body(reply.body.into_bytes())
            .unwrap_or_else(|_| Response::new(vec![]))
    }
}

/// The admin API's service, on `address`
pub fn admin_service(address: SocketAddr, token: Option<String>) -> Service<HttpServer<AdminApi>> {
    let mut service = Service::new(
        "admin api".to_string(),
        HttpServer::new_app(AdminApi::new(token)),
    );
    service.add_tcp(&address.to_string());
    service.threads = Some(1);
    service
}

#[cfg(test)]
mod tests {
    use http::{Method, StatusCode};

    use super::AdminApi;

    #[test]
    fn test_metrics() {
        let api = AdminApi::new(Some("secret".to_string()));
        crate::metrics::LocationMetrics::unknown()
            .record(404, std::time::Duration::from_millis(1));

        let reply = api.handle(&Method::GET, "/metrics", Some("Bearer secret"));

        assert_eq!(reply.status, StatusCode::OK);
        assert_eq!(reply.content_type, "text/plain; version=0.0.4");
        assert!(reply.body.contains("proksi_requests_total"), "{}", reply.body);
    }

    #[test]
    fn test_token() {
        let api = AdminApi::new(Some("secret".to_string()));

        for authorization in [None, Some("Bearer wrong"), Some("secret"), Some("Basic secret")] {
            let reply = api.handle(&Method::GET, "/metrics", authorization);
            assert_eq!(reply.status, StatusCode::UNAUTHORIZED, "{authorization:?}");
            assert_eq!(reply.body, r#"{"error":"missing or invalid token"}"#);
        }
    }

    #[test]
    fn test_without_a_token() {
        // Only on a loopback address (checked when the configuration loads)
        let api = AdminApi::new(None);

        assert_eq!(api.handle(&Method::GET, "/metrics", None).status, StatusCode::OK);
    }

    #[test]
    fn test_unknown_paths_and_methods() {
        let api = AdminApi::new(None);

        let reply = api.handle(&Method::GET, "/nope", None);
        assert_eq!(reply.status, StatusCode::NOT_FOUND);
        assert_eq!(reply.body, r#"{"error":"not found"}"#);
        assert_eq!(
            api.handle(&Method::POST, "/metrics", None).status,
            StatusCode::METHOD_NOT_ALLOWED
        );
    }
}
