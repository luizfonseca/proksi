//! `cors { origins = ["https://app.example.com"] }`: answers preflight requests and adds the CORS
//! headers to responses, for the origins a location accepts

use anyhow::Result;
use async_trait::async_trait;
use http::{header, HeaderValue, Method, StatusCode};
use pingora::{
    http::{RequestHeader, ResponseHeader},
    proxy::Session,
};

use crate::{
    config::RoutePlugin,
    proxy_server::{https_proxy::RouterContext, respond::respond},
};

use super::Middleware;

const DEFAULT_METHODS: &str = "GET, HEAD, POST, PUT, PATCH, DELETE";

/// Preflight requests (`OPTIONS` with `Access-Control-Request-Method`) are answered with a
/// `204`, with the CORS headers when their origin is accepted. Responses to other requests from
/// an accepted origin get `Access-Control-Allow-Origin`, replacing the upstream's.
pub(super) struct Cors {
    origins: Vec<Origin>,
    methods: HeaderValue,
    /// The request headers allowed (`None`: those a preflight asks for)
    headers: Option<HeaderValue>,
    expose_headers: Option<HeaderValue>,
    credentials: bool,
    max_age: Option<HeaderValue>,
}

#[derive(Debug, PartialEq)]
enum Origin {
    Any,
    Exact(String),
    /// `https://*.example.com`: the scheme, and the domain the subdomains are in
    Subdomains(String, String),
}

impl Origin {
    fn parse(origin: &str) -> Result<Self, String> {
        if origin == "*" {
            return Ok(Self::Any);
        }
        let invalid = || {
            format!(
                "cors: {origin:?} isn't an origin (\"https://app.example.com\", \
                 \"https://*.example.com\" or \"*\")"
            )
        };
        let (scheme, host) = origin.split_once("://").ok_or_else(invalid)?;
        if !matches!(scheme, "http" | "https") || host.is_empty() || host.contains('/') {
            return Err(invalid());
        }
        match host.strip_prefix('*') {
            Some(domain) if domain.starts_with('.') && !domain.contains('*') => Ok(
                Self::Subdomains(format!("{scheme}://"), domain.to_ascii_lowercase()),
            ),
            Some(_) => Err(invalid()),
            None if host.contains('*') => Err(invalid()),
            None => Ok(Self::Exact(origin.to_ascii_lowercase())),
        }
    }

    fn accepts(&self, origin: &str) -> bool {
        match self {
            Self::Any => true,
            Self::Exact(exact) => origin.eq_ignore_ascii_case(exact),
            Self::Subdomains(scheme, domain) => {
                let origin = origin.to_ascii_lowercase();
                origin
                    .strip_prefix(scheme.as_str())
                    .and_then(|host| host.strip_suffix(domain.as_str()))
                    .is_some_and(|subdomain| {
                        !subdomain.is_empty() && !subdomain.contains(['/', ':'])
                    })
            }
        }
    }
}

impl Cors {
    pub(super) fn new(plugin: &RoutePlugin) -> Result<Self, String> {
        let settings: super::settings::Cors = super::settings::read("cors", plugin)?;
        // A list as a header value (`GET, POST`)
        let joined = |key: &str, values: Option<Vec<String>>| -> Result<Option<HeaderValue>, String> {
            values
                .filter(|values| !values.is_empty())
                .map(|values| {
                    HeaderValue::from_str(&values.join(", "))
                        .map_err(|_| format!("cors: `{key}` has an invalid value"))
                })
                .transpose()
        };

        if settings.origins.is_empty() {
            return Err("cors: `origins` is required".to_string());
        }
        let origins = settings
            .origins
            .iter()
            .map(|origin| Origin::parse(origin))
            .collect::<Result<Vec<_>, _>>()?;

        Ok(Self {
            origins,
            methods: joined("methods", settings.methods)?
                .unwrap_or(HeaderValue::from_static(DEFAULT_METHODS)),
            headers: joined("headers", settings.headers)?,
            expose_headers: joined("expose_headers", settings.expose_headers)?,
            credentials: settings.credentials.unwrap_or(false),
            max_age: settings.max_age_secs.map(HeaderValue::from),
        })
    }

    /// `Access-Control-Allow-Origin` for a request from `origin`, if it's accepted. `*` unless
    /// credentials are allowed (browsers then need the origin itself).
    fn allow_origin(&self, origin: &HeaderValue) -> Option<HeaderValue> {
        let text = origin.to_str().ok()?;
        let accepted = self.origins.iter().find(|allowed| allowed.accepts(text))?;
        if *accepted == Origin::Any && !self.credentials {
            Some(HeaderValue::from_static("*"))
        } else {
            Some(origin.clone())
        }
    }

    /// Whether the response depends on the request's origin (`Vary: Origin`)
    fn varies(&self) -> bool {
        self.credentials || self.origins.iter().any(|origin| *origin != Origin::Any)
    }
}

fn is_preflight(request: &RequestHeader) -> bool {
    request.method == Method::OPTIONS
        && request
            .headers
            .contains_key(header::ACCESS_CONTROL_REQUEST_METHOD)
}

#[async_trait]
impl Middleware for Cors {
    async fn request(&self, session: &mut Session, ctx: &mut RouterContext) -> Result<bool> {
        let request = session.req_header();
        if !is_preflight(request) {
            return Ok(false);
        }
        let asked_headers = request
            .headers
            .get(header::ACCESS_CONTROL_REQUEST_HEADERS)
            .cloned();
        let allowed = request
            .headers
            .get(header::ORIGIN)
            .is_some_and(|origin| self.allow_origin(origin).is_some());

        let mut response = ResponseHeader::build_no_case(StatusCode::NO_CONTENT, Some(5))?;
        if allowed {
            response.insert_header(header::ACCESS_CONTROL_ALLOW_METHODS, self.methods.clone())?;
            if let Some(headers) = self.headers.clone().or(asked_headers) {
                response.insert_header(header::ACCESS_CONTROL_ALLOW_HEADERS, headers)?;
            }
            if let Some(max_age) = &self.max_age {
                response.insert_header(header::ACCESS_CONTROL_MAX_AGE, max_age.clone())?;
            }
        }
        // `Access-Control-Allow-Origin` and the rest: added by the response stage
        respond(session, ctx, response).await?;
        Ok(true)
    }

    async fn response(
        &self,
        session: &mut Session,
        response: &mut ResponseHeader,
        _ctx: &mut RouterContext,
    ) -> Result<()> {
        if self.varies() {
            response.append_header(header::VARY, "Origin")?;
        }
        let request = session.req_header();
        let Some(allow_origin) = request
            .headers
            .get(header::ORIGIN)
            .and_then(|origin| self.allow_origin(origin))
        else {
            return Ok(());
        };

        response.insert_header(header::ACCESS_CONTROL_ALLOW_ORIGIN, allow_origin)?;
        if self.credentials {
            response.insert_header(header::ACCESS_CONTROL_ALLOW_CREDENTIALS, "true")?;
        }
        if let Some(expose) = self
            .expose_headers
            .as_ref()
            .filter(|_| !is_preflight(request))
        {
            response.insert_header(header::ACCESS_CONTROL_EXPOSE_HEADERS, expose.clone())?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::borrow::Cow;

    use http::HeaderValue;

    use crate::config::RoutePlugin;

    use super::{Cors, Origin};

    fn cors(config: serde_json::Value) -> Result<Cors, String> {
        Cors::new(&RoutePlugin {
            name: Cow::Borrowed("cors"),
            config: serde_json::from_value(config).unwrap(),
        })
    }

    fn allows(cors: &Cors, origin: &str) -> Option<String> {
        cors.allow_origin(&HeaderValue::from_str(origin).unwrap())
            .map(|value| value.to_str().unwrap().to_string())
    }

    #[test]
    fn test_origins() {
        let cors = cors(serde_json::json!({
            "origins": ["https://app.example.com", "https://*.preview.example.com"]
        }))
        .unwrap();

        assert_eq!(
            allows(&cors, "https://app.example.com").as_deref(),
            Some("https://app.example.com")
        );
        assert_eq!(
            allows(&cors, "https://APP.example.com").as_deref(),
            Some("https://APP.example.com")
        );
        assert!(allows(&cors, "https://a.b.preview.example.com").is_some());
        for refused in [
            "http://app.example.com",
            "https://app.example.com:8443",
            "https://preview.example.com",
            "https://evilpreview.example.com",
            "https://x.preview.example.com:1",
            "null",
        ] {
            assert_eq!(allows(&cors, refused), None, "{refused}");
        }
    }

    #[test]
    fn test_any_origin() {
        let any = cors(serde_json::json!({ "origins": ["*"] })).unwrap();
        assert_eq!(allows(&any, "https://a.test").as_deref(), Some("*"));
        assert!(!any.varies());

        // Credentials need the origin itself
        let any = cors(serde_json::json!({ "origins": ["*"], "credentials": true })).unwrap();
        assert_eq!(
            allows(&any, "https://a.test").as_deref(),
            Some("https://a.test")
        );
        assert!(any.varies());
    }

    #[test]
    fn test_allowed_origins_of_earlier_configurations() {
        // `plugins = [{ name = "cors", config = { allowed_origins = [...] } }]` was accepted (and
        // ignored) before the block existed
        let cors = cors(serde_json::json!({ "allowed_origins": ["https://a.test"] })).unwrap();
        assert!(allows(&cors, "https://a.test").is_some());
    }

    #[test]
    fn test_invalid_settings() {
        assert_eq!(
            cors(serde_json::json!({})).err().as_deref(),
            Some("cors: `origins` is required")
        );
        for origin in [
            "app.example.com",
            "https://a.test/path",
            "ftp://a.test",
            "https://a.*.test",
        ] {
            assert!(Origin::parse(origin).is_err(), "{origin}");
        }
        assert!(cors(serde_json::json!({ "origins": ["*"], "credentials": "yes" })).is_err());
        assert!(cors(serde_json::json!({ "origins": ["*"], "methods": "GET" })).is_err());
        assert!(cors(serde_json::json!({ "origins": ["*"], "max_age_secs": -1 })).is_err());
    }
}
