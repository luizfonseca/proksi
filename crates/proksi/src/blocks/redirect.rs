//! `redirect { to = "https://example.com{uri}" }`: answers with a redirect, and
//! `respond { status = 503, body = "..." }`: answers with a fixed response

use anyhow::Result;
use async_trait::async_trait;
use bytes::Bytes;
use http::header;
use pingora::{http::ResponseHeader, proxy::Session};

use crate::{
    config::RoutePlugin,
    proxy_server::{
        https_proxy::RouterContext,
        respond::{respond, respond_with_body},
    },
};

use super::Middleware;

const REDIRECT_STATUSES: [u16; 5] = [301, 302, 303, 307, 308];

/// Answers every request with a redirect to `to`, where `{uri}` (the path and query), `{path}`,
/// `{query}` and `{host}` (without its port) are those of the request
pub(super) struct Redirect {
    to: Vec<Part>,
    status: u16,
}

#[derive(Debug, PartialEq)]
enum Part {
    Text(String),
    Uri,
    Path,
    Query,
    Host,
}

impl Redirect {
    pub(super) fn new(plugin: &RoutePlugin) -> Result<Self, String> {
        let settings: super::settings::Redirect = super::settings::read("redirect", plugin)?;
        if settings.to.is_empty() {
            return Err("redirect: `to` is required".to_string());
        }
        let status = match settings.status {
            None => 302,
            Some(status) => Some(status)
                .filter(|status| REDIRECT_STATUSES.contains(status))
                .ok_or("redirect: `status` must be 301, 302, 303, 307 or 308")?,
        };
        Ok(Self {
            to: parse(&settings.to)?,
            status,
        })
    }

    /// Where a request for `path`, `query` and `host` is sent
    fn location(&self, path: &str, query: Option<&str>, host: &str) -> String {
        let mut location = String::new();
        for part in &self.to {
            match part {
                Part::Text(text) => location.push_str(text),
                Part::Uri => {
                    location.push_str(path);
                    if let Some(query) = query {
                        location.push('?');
                        location.push_str(query);
                    }
                }
                Part::Path => location.push_str(path),
                Part::Query => location.push_str(query.unwrap_or_default()),
                Part::Host => location.push_str(host),
            }
        }
        location
    }
}

/// `to`, split into text and placeholders
fn parse(to: &str) -> Result<Vec<Part>, String> {
    let mut parts = vec![];
    let mut rest = to;
    while let Some(start) = rest.find('{') {
        let (text, placeholder) = rest.split_at(start);
        if !text.is_empty() {
            parts.push(Part::Text(text.to_string()));
        }
        let end = placeholder
            .find('}')
            .ok_or_else(|| format!("redirect: `to` has an unclosed `{{` ({to:?})"))?;
        parts.push(match placeholder.get(1..end).unwrap_or_default() {
            "uri" => Part::Uri,
            "path" => Part::Path,
            "query" => Part::Query,
            "host" => Part::Host,
            other => {
                return Err(format!(
                    "redirect: `{{{other}}}` isn't a placeholder of `to` (`{{uri}}`, `{{path}}`, \
                     `{{query}}` or `{{host}}`)"
                ))
            }
        });
        rest = placeholder.get(end + 1..).unwrap_or_default();
    }
    if !rest.is_empty() {
        parts.push(Part::Text(rest.to_string()));
    }
    Ok(parts)
}

#[async_trait]
impl Middleware for Redirect {
    async fn request(&self, session: &mut Session, ctx: &mut RouterContext) -> Result<bool> {
        let uri = &session.req_header().uri;
        let location = self.location(uri.path(), uri.query(), &ctx.host);

        let mut response = ResponseHeader::build_no_case(self.status, Some(3))?;
        response.insert_header(header::LOCATION, location)?;
        response.insert_header(header::CONTENT_LENGTH, "0")?;
        respond(session, ctx, response).await?;
        Ok(true)
    }
}

/// Answers every request with `status`, `body` and its `content_type`
pub(super) struct Respond {
    status: u16,
    body: Bytes,
    content_type: Option<String>,
}

impl Respond {
    pub(super) fn new(plugin: &RoutePlugin) -> Result<Self, String> {
        let settings: super::settings::Respond = super::settings::read("respond", plugin)?;
        let status = match settings.status {
            None => 200,
            Some(status) => Some(status)
                .filter(|status| (200..600).contains(status))
                .ok_or("respond: `status` must be between 200 and 599")?,
        };
        if let Some(content_type) = &settings.content_type {
            http::HeaderValue::from_str(content_type)
                .map_err(|_| format!("respond: {content_type:?} isn't a valid content type"))?;
        }
        Ok(Self {
            status,
            body: Bytes::from(settings.body.unwrap_or_default()),
            content_type: settings.content_type,
        })
    }
}

#[async_trait]
impl Middleware for Respond {
    async fn request(&self, session: &mut Session, ctx: &mut RouterContext) -> Result<bool> {
        let mut response = ResponseHeader::build_no_case(self.status, Some(3))?;
        if let Some(content_type) = &self.content_type {
            response.insert_header(header::CONTENT_TYPE, content_type.as_str())?;
        }
        respond_with_body(session, ctx, response, self.body.clone()).await?;
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use std::borrow::Cow;

    use crate::config::RoutePlugin;

    use super::{Redirect, Respond};

    fn plugin(name: &'static str, config: serde_json::Value) -> RoutePlugin {
        RoutePlugin {
            name: Cow::Borrowed(name),
            config: serde_json::from_value(config).unwrap(),
        }
    }

    fn redirect(config: serde_json::Value) -> Result<Redirect, String> {
        Redirect::new(&plugin("redirect", config))
    }

    #[test]
    fn test_redirect_placeholders() {
        let to = redirect(serde_json::json!({ "to": "https://{host}.example.com{uri}" })).unwrap();
        assert_eq!(to.location("/a", Some("b=1"), "www"), "https://www.example.com/a?b=1");
        assert_eq!(to.location("/a", None, "www"), "https://www.example.com/a");
        assert_eq!(to.status, 302);

        let to = redirect(serde_json::json!({ "to": "/new{path}?{query}", "status": 308 }))
            .unwrap();
        assert_eq!(to.location("/old", Some("x=1"), "a.test"), "/new/old?x=1");
        assert_eq!(to.status, 308);

        let to = redirect(serde_json::json!({ "to": "https://example.com" })).unwrap();
        assert_eq!(to.location("/a", Some("b"), "a.test"), "https://example.com");
    }

    #[test]
    fn test_invalid_redirects() {
        for (config, err) in [
            (serde_json::json!({}), "redirect: `to` is required"),
            (
                serde_json::json!({ "to": "/x", "status": 200 }),
                "redirect: `status` must be 301, 302, 303, 307 or 308",
            ),
            (
                serde_json::json!({ "to": "/{request_uri}" }),
                "redirect: `{request_uri}` isn't a placeholder of `to` (`{uri}`, `{path}`, \
                 `{query}` or `{host}`)",
            ),
            (
                serde_json::json!({ "to": "/{uri" }),
                "redirect: `to` has an unclosed `{` (\"/{uri\")",
            ),
        ] {
            assert_eq!(redirect(config).err().as_deref(), Some(err));
        }
    }

    #[test]
    fn test_respond_settings() {
        let respond = Respond::new(&plugin("respond", serde_json::json!({}))).unwrap();
        assert_eq!((respond.status, respond.body.len()), (200, 0));

        assert!(Respond::new(&plugin("respond", serde_json::json!({ "status": 99 }))).is_err());
        assert!(Respond::new(&plugin("respond", serde_json::json!({ "body": 1 }))).is_err());
        assert!(
            Respond::new(&plugin("respond", serde_json::json!({ "content_type": "a\nb" })))
                .is_err()
        );
    }
}
