//! `forward_auth { url = "http://auth:9091/verify" }`: asks an auth service whether each request
//! may go through, as nginx's `auth_request` and Traefik's `forwardAuth` (Authelia, Authentik,
//! oauth2-proxy...)

use std::{sync::OnceLock, time::Duration};

use anyhow::Result;
use async_trait::async_trait;
use bytes::{Bytes, BytesMut};
use http::{header, HeaderMap, HeaderName, HeaderValue};
use pingora::{http::ResponseHeader, proxy::Session};

use crate::{
    config::RoutePlugin,
    proxy_server::{
        https_proxy::RouterContext,
        respond::{respond_error, respond_with_body},
    },
};

use super::Middleware;

/// The largest answer of an auth service passed on to the client (a login page)
const MAX_BODY: usize = 1024 * 1024;

/// Headers of a connection, not of the request or response itself
const HOP_BY_HOP: [HeaderName; 8] = [
    header::CONNECTION,
    header::CONTENT_LENGTH,
    header::HOST,
    header::PROXY_AUTHENTICATE,
    header::PROXY_AUTHORIZATION,
    header::TE,
    header::TRAILER,
    header::TRANSFER_ENCODING,
];

/// Every request is first sent to `url` (`GET`, with the request's headers and
/// `X-Forwarded-Method`, `-Proto`, `-Host`, `-Uri` and `-For`). A `2xx` lets it through, with
/// the `response_headers` of the answer (replacing the client's own); any other answer is sent
/// back to the client as it is (e.g. a redirect to a login page). `502` when the auth service
/// can't be reached or doesn't answer within `timeout_secs`.
pub(super) struct ForwardAuth {
    url: reqwest::Url,
    /// The request headers sent to the auth service (`None`: all of them)
    request_headers: Option<Vec<HeaderName>>,
    response_headers: Vec<HeaderName>,
    timeout: Duration,
}

impl ForwardAuth {
    pub(super) fn new(plugin: &RoutePlugin) -> Result<Self, String> {
        let setting = |key: &str| plugin.config.as_ref().and_then(|config| config.get(key));
        let names = |key: &str| -> Result<Option<Vec<HeaderName>>, String> {
            setting(key)
                .map(|value| {
                    let invalid =
                        || format!("forward_auth: `{key}` must be a list of header names");
                    value
                        .as_array()
                        .ok_or_else(invalid)?
                        .iter()
                        .map(|name| {
                            name.as_str()
                                .and_then(|name| HeaderName::from_bytes(name.as_bytes()).ok())
                                .ok_or_else(invalid)
                        })
                        .collect()
                })
                .transpose()
        };

        let url = setting("url")
            .and_then(serde_json::Value::as_str)
            .ok_or("forward_auth: `url` is required")?;
        let url = reqwest::Url::parse(url)
            .ok()
            .filter(|url| matches!(url.scheme(), "http" | "https"))
            .ok_or_else(|| format!("forward_auth: {url:?} isn't an http or https URL"))?;
        let timeout = match setting("timeout_secs") {
            None => Duration::from_secs(5),
            Some(value) => value
                .as_u64()
                .filter(|secs| *secs > 0)
                .map(Duration::from_secs)
                .ok_or("forward_auth: `timeout_secs` must be a number greater than 0")?,
        };

        Ok(Self {
            url,
            request_headers: names("request_headers")?,
            response_headers: names("response_headers")?.unwrap_or_default(),
            timeout,
        })
    }

    /// The headers of the request sent to the auth service
    fn auth_headers(&self, session: &Session) -> HeaderMap {
        let request = session.req_header();
        let mut headers = HeaderMap::with_capacity(request.headers.len() + 5);
        for (name, value) in &request.headers {
            let wanted = match &self.request_headers {
                Some(names) => names.contains(name),
                None => !HOP_BY_HOP.contains(name),
            };
            if wanted {
                headers.append(name.clone(), value.clone());
            }
        }

        let tls = session
            .digest()
            .is_some_and(|digest| digest.ssl_digest.is_some());
        let forwarded: [(&str, Option<HeaderValue>); 5] = [
            (
                "x-forwarded-method",
                Some(HeaderValue::from_str(request.method.as_str()).ok()).flatten(),
            ),
            (
                "x-forwarded-proto",
                Some(HeaderValue::from_static(if tls { "https" } else { "http" })),
            ),
            (
                "x-forwarded-host",
                request.headers.get(header::HOST).cloned().or_else(|| {
                    request
                        .uri
                        .host()
                        .and_then(|host| HeaderValue::from_str(host).ok())
                }),
            ),
            (
                "x-forwarded-uri",
                request
                    .uri
                    .path_and_query()
                    .and_then(|uri| HeaderValue::from_str(uri.as_str()).ok()),
            ),
            (
                "x-forwarded-for",
                crate::proxy_server::client::of_session(session)
                    .and_then(|client| HeaderValue::from_str(&client.to_string()).ok()),
            ),
        ];
        for (name, value) in forwarded {
            if let Some(value) = value {
                headers.insert(HeaderName::from_static(name), value);
            }
        }
        headers
    }
}

/// One client for every `forward_auth` block, created with the first one (each client loads its
/// own copy of the system's trusted certificates)
fn client() -> &'static reqwest::Client {
    static CLIENT: OnceLock<reqwest::Client> = OnceLock::new();
    CLIENT.get_or_init(|| {
        reqwest::Client::builder()
            // Redirects are the client's to follow (to a login page)
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .unwrap_or_default()
    })
}

/// The body of `response`, unless it's larger than [`MAX_BODY`]
async fn body(mut response: reqwest::Response) -> Result<Bytes, String> {
    let mut body = BytesMut::new();
    while let Some(chunk) = response.chunk().await.map_err(|err| err.to_string())? {
        if body.len() + chunk.len() > MAX_BODY {
            return Err("its answer is larger than 1MiB".to_string());
        }
        body.extend_from_slice(&chunk);
    }
    Ok(body.freeze())
}

#[async_trait]
impl Middleware for ForwardAuth {
    async fn request(&self, session: &mut Session, ctx: &mut RouterContext) -> Result<bool> {
        let checked = client()
            .get(self.url.clone())
            .headers(self.auth_headers(session))
            .timeout(self.timeout)
            .send()
            .await;
        let response = match checked {
            Ok(response) => response,
            Err(err) => {
                tracing::warn!(url = %self.url, "forward_auth: {err}");
                respond_error(session, ctx, 502).await?;
                return Ok(true);
            }
        };

        // Through, with the headers of the answer (never the client's own)
        if response.status().is_success() {
            let request = session.req_header_mut();
            for name in &self.response_headers {
                request.remove_header(name);
                for value in response.headers().get_all(name) {
                    request.append_header(name.clone(), value.clone())?;
                }
            }
            return Ok(false);
        }

        // The auth service's answer, to the client
        let mut answer = ResponseHeader::build(response.status().as_u16(), None)?;
        for (name, value) in response.headers() {
            if !HOP_BY_HOP.contains(name) {
                answer.append_header(name.clone(), value.clone())?;
            }
        }
        match body(response).await {
            Ok(body) => respond_with_body(session, ctx, answer, body).await?,
            Err(err) => {
                tracing::warn!(url = %self.url, "forward_auth: {err}");
                respond_error(session, ctx, 502).await?;
            }
        }
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use std::{borrow::Cow, time::Duration};

    use crate::config::RoutePlugin;

    use super::ForwardAuth;

    fn forward_auth(config: serde_json::Value) -> Result<ForwardAuth, String> {
        ForwardAuth::new(&RoutePlugin {
            name: Cow::Borrowed("forward_auth"),
            config: serde_json::from_value(config).unwrap(),
        })
    }

    #[test]
    fn test_settings() {
        let auth = forward_auth(serde_json::json!({
            "url": "http://auth:9091/verify",
            "response_headers": ["Remote-User", "remote-groups"],
            "request_headers": ["Cookie"],
        }))
        .unwrap();
        assert_eq!(auth.url.as_str(), "http://auth:9091/verify");
        assert_eq!(auth.response_headers, ["remote-user", "remote-groups"]);
        assert_eq!(auth.request_headers.unwrap(), ["cookie"]);
        assert_eq!(auth.timeout, Duration::from_secs(5));
    }

    #[test]
    fn test_invalid_settings() {
        for (config, err) in [
            (serde_json::json!({}), "forward_auth: `url` is required"),
            (
                serde_json::json!({ "url": "auth:9091" }),
                "forward_auth: \"auth:9091\" isn't an http or https URL",
            ),
            (
                serde_json::json!({ "url": "http://a", "response_headers": ["bad header"] }),
                "forward_auth: `response_headers` must be a list of header names",
            ),
            (
                serde_json::json!({ "url": "http://a", "timeout_secs": 0 }),
                "forward_auth: `timeout_secs` must be a number greater than 0",
            ),
        ] {
            assert_eq!(forward_auth(config).err().as_deref(), Some(err));
        }
    }
}
