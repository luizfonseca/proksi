//! `jwt { ... }`: requests need a valid JSON Web Token, from `Authorization: Bearer` or a
//! cookie. Others are answered `401`. Claims can be sent to the upstream as headers.

pub mod jwks;
mod verifier;

use anyhow::Result;
use async_trait::async_trait;
use http::{header, HeaderName, HeaderValue};
use pingora::{http::ResponseHeader, proxy::Session};
use serde_json::{Map, Value};

use crate::{respond::respond, RequestCtx, RoutePlugin};

use super::{settings, Middleware};
use verifier::{Keys, Verifier};

pub(super) struct Jwt {
    verifier: Verifier,
    cookie: Option<String>,
    /// Claim, and the header it's sent as
    claims_to_headers: Vec<(String, HeaderName)>,
}

impl Jwt {
    pub(super) fn new(plugin: &RoutePlugin) -> Result<Self, String> {
        let settings: settings::Jwt = settings::read("jwt", plugin)?;
        let keys = match (settings.secret, settings.public_key, settings.jwks_url) {
            (Some(secret), None, None) if !secret.is_empty() => Keys::secret(&secret),
            (None, Some(pem), None) => {
                Keys::public_key(&pem).map_err(|err| format!("jwt: {err}"))?
            }
            (None, None, Some(url)) => {
                if !(url.starts_with("https://") || url.starts_with("http://")) {
                    return Err("jwt: jwks_url must be an http or https URL".to_string());
                }
                Keys::Jwks(jwks::source(&url))
            }
            _ => return Err("jwt: one of secret, public_key or jwks_url is required".to_string()),
        };
        let claims_to_headers = settings
            .claims_to_headers
            .unwrap_or_default()
            .into_iter()
            .map(|(claim, name)| {
                HeaderName::from_bytes(name.as_bytes())
                    .map(|name| (claim, name))
                    .map_err(|_| format!("jwt: claims_to_headers: \"{name}\" is not a header name"))
            })
            .collect::<Result<_, _>>()?;

        Ok(Self {
            verifier: Verifier::new(keys, settings.issuer, settings.audience),
            cookie: settings.cookie,
            claims_to_headers,
        })
    }

    /// The token of the request, from its cookie or `Authorization: Bearer`
    fn token<'a>(&self, session: &'a Session) -> Option<&'a str> {
        let headers = &session.req_header().headers;
        match &self.cookie {
            Some(cookie) => headers
                .get_all(header::COOKIE)
                .iter()
                .filter_map(|value| value.to_str().ok())
                .flat_map(|value| value.split(';'))
                .find_map(|pair| {
                    let (name, value) = pair.trim().split_once('=')?;
                    (name == cookie).then_some(value)
                }),
            None => headers
                .get(header::AUTHORIZATION)
                .and_then(|value| value.to_str().ok())
                .and_then(|value| {
                    let (scheme, token) = value.split_once(' ')?;
                    scheme.eq_ignore_ascii_case("bearer").then(|| token.trim())
                }),
        }
        .filter(|token| !token.is_empty())
    }

    /// The claims sent as headers, replacing any the client sent
    fn claims_to_headers(&self, session: &mut Session, claims: &Map<String, Value>) {
        let request = session.req_header_mut();
        for (claim, name) in &self.claims_to_headers {
            request.remove_header(name);
            let value = claims.get(claim).and_then(header_value);
            if let Some(value) = value.and_then(|value| HeaderValue::from_str(&value).ok()) {
                let _ = request.insert_header(name.clone(), value);
            }
        }
    }
}

/// A claim as a header's value: lists joined with commas, nothing for `null`
fn header_value(claim: &Value) -> Option<String> {
    let text = |value: &Value| match value {
        Value::String(text) => text.clone(),
        other => other.to_string(),
    };
    match claim {
        Value::Null => None,
        Value::Array(values) => Some(values.iter().map(text).collect::<Vec<_>>().join(",")),
        other => Some(text(other)),
    }
}

/// `401`, saying a bearer token is needed (RFC 6750): the reason isn't given, only logged
async fn unauthorized(
    session: &mut Session,
    ctx: &mut dyn RequestCtx,
    invalid: bool,
) -> Result<()> {
    let mut response = ResponseHeader::build(401, Some(3))?;
    let challenge = if invalid {
        "Bearer error=\"invalid_token\""
    } else {
        "Bearer"
    };
    response.insert_header(header::WWW_AUTHENTICATE, challenge)?;
    response.insert_header(header::CONTENT_LENGTH, "0")?;
    response.insert_header(header::CACHE_CONTROL, "private, no-store")?;
    respond(session, ctx, response).await?;
    Ok(())
}

#[async_trait]
impl Middleware for Jwt {
    async fn request(&self, session: &mut Session, ctx: &mut dyn RequestCtx) -> Result<bool> {
        let Some(token) = self.token(session) else {
            unauthorized(session, ctx, false).await?;
            return Ok(true);
        };
        match self.verifier.verify(token) {
            Ok(claims) => {
                self.claims_to_headers(session, &claims);
                Ok(false)
            }
            Err(err) => {
                tracing::debug!("jwt: {err}");
                unauthorized(session, ctx, true).await?;
                Ok(true)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::borrow::Cow;

    use crate::RoutePlugin;

    use super::Jwt;

    fn jwt(config: serde_json::Value) -> Result<Jwt, String> {
        Jwt::new(&RoutePlugin {
            name: Cow::Borrowed("jwt"),
            config: serde_json::from_value(config).unwrap(),
        })
    }

    #[test]
    fn test_one_kind_of_key() {
        assert!(jwt(serde_json::json!({ "secret": "s" })).is_ok());
        assert!(jwt(serde_json::json!({ "jwks_url": "https://auth.test/jwks.json" })).is_ok());

        let required = Some("jwt: one of secret, public_key or jwks_url is required".to_string());
        assert_eq!(jwt(serde_json::json!({})).err(), required);
        assert_eq!(jwt(serde_json::json!({ "secret": "" })).err(), required);
        assert_eq!(
            jwt(serde_json::json!({ "secret": "s", "jwks_url": "https://auth.test/" })).err(),
            required
        );
        assert_eq!(
            jwt(serde_json::json!({ "jwks_url": "auth.test/jwks.json" })).err(),
            Some("jwt: jwks_url must be an http or https URL".to_string())
        );
        assert!(jwt(serde_json::json!({ "public_key": "nope" })).is_err());
    }

    #[test]
    fn test_header_names() {
        let config =
            serde_json::json!({ "secret": "s", "claims_to_headers": { "sub": "bad header" } });
        assert_eq!(
            jwt(config).err(),
            Some("jwt: claims_to_headers: \"bad header\" is not a header name".to_string())
        );
        assert!(jwt(
            serde_json::json!({ "secret": "s", "claims_to_headers": { "sub": "X-User-Id" } })
        )
        .is_ok());
        assert!(jwt(serde_json::json!({ "secret": "s", "unknown": 1 })).is_err());
    }
}
