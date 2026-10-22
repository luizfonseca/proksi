//! `body_rewrite { json { set = { ... }, remove = [...] }, replace { pattern, with } }`: changes
//! the request body before it's sent upstream. The body is read whole first (up to `max_size`,
//! `413` beyond), and sent with its new `Content-Length`.

use std::borrow::Cow;

use anyhow::Result;
use async_trait::async_trait;
use bytes::Bytes;
use http::{header, StatusCode};
use pingora::{http::ResponseHeader, proxy::Session};

use crate::{
    body::{self, Unread},
    body_limit::parse_size,
    respond::respond,
    RequestCtx, RoutePlugin,
};

use super::Middleware;

use json::Edit;

mod json;
mod path;

const DEFAULT_MAX_SIZE: u64 = 1024 * 1024;

/// Compiled patterns are limited, as for regex locations
const REGEX_SIZE_LIMIT: usize = 1024 * 1024;

pub(crate) struct BodyRewrite {
    max_size: u64,
    /// Changes to JSON bodies, in order
    edits: Vec<Edit>,
    replaces: Vec<(regex::bytes::Regex, Vec<u8>)>,
}

impl BodyRewrite {
    pub(crate) fn new(plugin: &RoutePlugin) -> Result<Self, String> {
        let settings: super::settings::BodyRewrite = super::settings::read("body_rewrite", plugin)?;
        let in_block = |err: String| format!("body_rewrite: {err}");
        let max_size = match &settings.max_size {
            None => DEFAULT_MAX_SIZE,
            Some(size) => parse_size(size).ok_or_else(|| {
                in_block(format!(
                    "max_size {size:?} is not a size such as \"512KiB\" or \"1MiB\""
                ))
            })?,
        };

        let json = settings.json.unwrap_or_default();
        let sets = json
            .set
            .iter()
            .flatten()
            .map(|(path, value)| Edit::set(path, value));
        let removes = json.remove.iter().flatten().map(|path| Edit::remove(path));
        let edits = sets
            .chain(removes)
            .collect::<Result<Vec<_>, _>>()
            .map_err(in_block)?;

        let replaces = settings
            .replace
            .into_iter()
            .map(|replace| {
                let regex = regex::bytes::RegexBuilder::new(&replace.pattern)
                    .size_limit(REGEX_SIZE_LIMIT)
                    .build()
                    .map_err(|err| in_block(format!("pattern {:?}: {err}", replace.pattern)))?;
                Ok((regex, replace.with.into_bytes()))
            })
            .collect::<Result<Vec<_>, String>>()?;

        if edits.is_empty() && replaces.is_empty() {
            return Err(in_block(
                "nothing to change: add `json` or `replace`".to_string(),
            ));
        }
        Ok(Self {
            max_size,
            edits,
            replaces,
        })
    }

    /// `body` changed, or `None` when nothing applies to it
    fn rewrite(&self, body: &[u8], is_json: bool) -> Option<Vec<u8>> {
        let mut changed: Option<Vec<u8>> = None;
        for edit in self.edits.iter().filter(|_| is_json) {
            let new = edit.apply(changed.as_deref().unwrap_or(body));
            changed = new.or(changed);
        }
        for (regex, with) in &self.replaces {
            if let Cow::Owned(new) =
                regex.replace_all(changed.as_deref().unwrap_or(body), with.as_slice())
            {
                changed = Some(new);
            }
        }
        changed
    }
}

/// `application/json`, or a type ending with `+json` (`application/merge-patch+json`)
fn is_json(session: &Session) -> bool {
    let content_type = session.req_header().headers.get(header::CONTENT_TYPE);
    let media_type = content_type
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.split(';').next())
        .map(str::trim)
        .unwrap_or_default();
    media_type.eq_ignore_ascii_case("application/json")
        || media_type
            .get(media_type.len().saturating_sub(5)..)
            .is_some_and(|suffix| suffix.eq_ignore_ascii_case("+json"))
}

#[async_trait]
impl Middleware for BodyRewrite {
    async fn request(&self, session: &mut Session, ctx: &mut dyn RequestCtx) -> Result<bool> {
        let body = match body::read(session, ctx, self.max_size).await {
            Ok(body) => body,
            Err(Unread::TooLarge) => {
                return refuse(session, ctx, StatusCode::PAYLOAD_TOO_LARGE).await
            }
            Err(Unread::Failed) => return refuse(session, ctx, StatusCode::BAD_REQUEST).await,
        };
        if let Some(changed) = self.rewrite(&body, is_json(session)) {
            ctx.set_request_body(Bytes::from(changed));
        }
        Ok(false)
    }
}

async fn refuse(
    session: &mut Session,
    ctx: &mut dyn RequestCtx,
    status: StatusCode,
) -> Result<bool> {
    let mut response = ResponseHeader::build_no_case(status, Some(1))?;
    response.insert_header(header::CONTENT_LENGTH, "0")?;
    respond(session, ctx, response).await?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use crate::RoutePlugin;

    use super::BodyRewrite;

    fn block(config: serde_json::Value) -> Result<BodyRewrite, String> {
        BodyRewrite::new(&RoutePlugin {
            name: "body_rewrite".into(),
            config: serde_json::from_value(config).unwrap(),
        })
    }

    fn rewrite(block: &BodyRewrite, body: &str, is_json: bool) -> Option<String> {
        block
            .rewrite(body.as_bytes(), is_json)
            .map(|out| String::from_utf8(out).unwrap())
    }

    #[test]
    fn test_settings() {
        let edits = block(serde_json::json!({
            "max_size": "64KiB",
            "json": { "set": { "$.role": "guest" }, "remove": ["$.debug"] },
            "replace": { "pattern": "secret-[0-9]+", "with": "[redacted]" },
        }))
        .unwrap();
        assert_eq!(edits.max_size, 64 * 1024);
        assert_eq!((edits.edits.len(), edits.replaces.len()), (2, 1));

        for (config, err) in [
            (
                serde_json::json!({}),
                "body_rewrite: nothing to change: add `json` or `replace`",
            ),
            (
                serde_json::json!({ "json": { "remove": ["$.items[*]"] } }),
                "body_rewrite: \"$.items[*]\": wildcards (`[*]`) aren't supported yet",
            ),
            (
                serde_json::json!({ "max_size": "big", "json": { "remove": ["$.a"] } }),
                "body_rewrite: max_size \"big\" is not a size such as \"512KiB\" or \"1MiB\"",
            ),
        ] {
            assert_eq!(block(config).err().as_deref(), Some(err));
        }
        let bad = block(serde_json::json!({ "replace": [{ "pattern": "(", "with": "" }] })).err();
        assert!(bad.is_some_and(|err| err.starts_with("body_rewrite: pattern \"(\":")));
    }

    #[test]
    fn test_json_changes_then_replacements_in_order() {
        let block = block(serde_json::json!({
            "json": { "set": { "$.user.role": "guest" }, "remove": ["$.debug"] },
            "replace": [
                { "pattern": "secret-([0-9]+)", "with": "hidden-$1" },
                { "pattern": "hidden", "with": "gone" },
            ],
        }))
        .unwrap();
        let body = r#"{"user": {"role": "admin", "note": "secret-42"}, "debug": true}"#;
        assert_eq!(
            rewrite(&block, body, true).as_deref(),
            Some(r#"{"user": {"role": "guest", "note": "gone-42"}}"#)
        );
        // Not JSON: only the replacements
        assert_eq!(
            rewrite(&block, "token=secret-7", false).as_deref(),
            Some("token=gone-7")
        );
        // Nothing to change
        assert_eq!(rewrite(&block, "plain", false), None);
    }
}
