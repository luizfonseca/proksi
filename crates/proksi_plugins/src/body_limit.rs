//! `body_limit { max = "1MiB" }`: answers `413` to requests with a larger body

use anyhow::Result;
use async_trait::async_trait;
use bytes::Bytes;
use http::{header, StatusCode};
use pingora::{http::ResponseHeader, proxy::Session, ErrorType::HTTPStatus};

use crate::{respond::respond, RequestCtx, RoutePlugin};

use super::Middleware;

const INVALID_MAX: &str =
    "body_limit: `max` must be a size such as \"512KiB\", \"1MiB\" or \"10MB\"";

/// Requests with a larger `Content-Length` are answered right away. Bodies without one
/// (chunked) are counted as they're proxied, and the request ends once it's over `max`.
pub(super) struct BodyLimit {
    max: u64,
}

impl BodyLimit {
    pub(super) fn new(plugin: &RoutePlugin) -> Result<Self, String> {
        let settings: super::settings::BodyLimit = super::settings::read("body_limit", plugin)?;
        let max = parse_size(&settings.max).ok_or(INVALID_MAX)?;
        Ok(Self { max })
    }
}

#[async_trait]
impl Middleware for BodyLimit {
    async fn request(&self, session: &mut Session, ctx: &mut dyn RequestCtx) -> Result<bool> {
        let content_length = session
            .req_header()
            .headers
            .get(header::CONTENT_LENGTH)
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.parse::<u64>().ok());
        if content_length.is_none_or(|length| length <= self.max) {
            return Ok(false);
        }

        let mut response = ResponseHeader::build_no_case(StatusCode::PAYLOAD_TOO_LARGE, Some(2))?;
        response.insert_header(header::CONTENT_LENGTH, "0")?;
        response.insert_header(header::CONNECTION, "close")?;
        session.set_keepalive(None);
        respond(session, ctx, response).await?;
        Ok(true)
    }

    fn request_body(
        &self,
        body: &mut Option<Bytes>,
        _end_of_stream: bool,
        ctx: &mut dyn RequestCtx,
    ) -> pingora::Result<()> {
        let read = ctx.count_request_body(body.as_ref().map_or(0, |body| body.len() as u64));
        if read > self.max {
            return Err(pingora::Error::explain(
                HTTPStatus(413),
                "request body larger than body_limit",
            ));
        }
        Ok(())
    }
}

/// A size like `512KiB`, `1MiB` or `10MB`
#[must_use]
pub fn parse_size(value: &str) -> Option<u64> {
    let split = value.find(|c: char| !c.is_ascii_digit())?;
    let (amount, unit) = value.split_at(split);
    let multiplier: u64 = match unit {
        "B" => 1,
        "KB" => 1000,
        "MB" => 1000 * 1000,
        "GB" => 1000 * 1000 * 1000,
        "KiB" => 1024,
        "MiB" => 1024 * 1024,
        "GiB" => 1024 * 1024 * 1024,
        _ => return None,
    };
    amount
        .parse::<u64>()
        .ok()
        .and_then(|amount| amount.checked_mul(multiplier))
        .filter(|size| *size > 0)
}

#[cfg(test)]
mod tests {
    use crate::RoutePlugin;

    use super::{parse_size, BodyLimit};

    fn plugin(config: serde_json::Value) -> RoutePlugin {
        serde_json::from_value(serde_json::json!({ "name": "body_limit", "config": config }))
            .unwrap()
    }

    #[test]
    fn test_parse_size() {
        assert_eq!(parse_size("100B"), Some(100));
        assert_eq!(parse_size("10KB"), Some(10_000));
        assert_eq!(parse_size("512KiB"), Some(512 * 1024));
        assert_eq!(parse_size("2MB"), Some(2_000_000));
        assert_eq!(parse_size("1MiB"), Some(1024 * 1024));
        assert_eq!(parse_size("1GiB"), Some(1024 * 1024 * 1024));
        for invalid in ["", "1", "MiB", "0B", "1.5MiB", "-1KB", "1TB", "1 MiB"] {
            assert_eq!(parse_size(invalid), None, "{invalid}");
        }
    }

    #[test]
    fn test_max() {
        let limit = BodyLimit::new(&plugin(serde_json::json!({ "max": "1KiB" }))).unwrap();
        assert_eq!(limit.max, 1024);
    }

    #[test]
    fn test_invalid_settings() {
        let err = |config| BodyLimit::new(&plugin(config)).err().unwrap_or_default();
        assert_eq!(err(serde_json::json!({})), "body_limit: `max` is required");
        assert_eq!(
            err(serde_json::json!({ "max": "1TB" })),
            "body_limit: `max` must be a size such as \"512KiB\", \"1MiB\" or \"10MB\""
        );
        assert!(err(serde_json::json!({ "max": 1024 })).starts_with("body_limit: invalid type"));
    }
}
