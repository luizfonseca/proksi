//! `body_rewrite { json { set = { ... }, remove = [...] }, replace { pattern, with } }`: changes
//! the request body as it's proxied, a part at a time, without holding it: JSON values by path
//! (`stream`), then regular expressions (`replace`). The new body's length isn't known when the
//! request is sent upstream, so it's sent without `Content-Length` (chunked to HTTP/1.1
//! upstreams).

use anyhow::Result;
use async_trait::async_trait;
use bytes::Bytes;
use http::{header, Version};
use pingora::{http::RequestHeader, proxy::Session};

use crate::{block_state, body_limit::parse_size, set_block_state, RequestCtx, RoutePlugin};

use super::Middleware;

use path::Step;
use replace::{Replace, Replacing};
use stream::{Action, Editor, Target};

mod path;
mod replace;
mod stream;

const DEFAULT_MAX_MATCH: usize = 1024;

/// Compiled patterns are limited, as for regex locations
const REGEX_SIZE_LIMIT: usize = 1024 * 1024;

pub(crate) struct BodyRewrite {
    /// Changes to JSON bodies
    targets: Vec<Target>,
    replaces: Vec<Replace>,
}

/// Where the changes are in a request's body
struct Rewriting {
    json: Option<Editor>,
    replacing: Vec<Replacing>,
}

impl BodyRewrite {
    pub(crate) fn new(plugin: &RoutePlugin) -> Result<Self, String> {
        let settings: super::settings::BodyRewrite = super::settings::read("body_rewrite", plugin)?;
        let in_block = |err: String| format!("body_rewrite: {err}");

        let json = settings.json.unwrap_or_default();
        let sets = json.set.iter().flatten().map(|(path, value)| {
            let value = serde_json::to_vec(value).map_err(|err| err.to_string())?;
            target(path, Action::Set(value))
        });
        let removes = json
            .remove
            .iter()
            .flatten()
            .map(|path| target(path, Action::Remove));
        let targets = sets
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
                    .map_err(|err| format!("pattern {:?}: {err}", replace.pattern))?;
                let max_match = match &replace.max_match {
                    None => DEFAULT_MAX_MATCH,
                    Some(size) => parse_size(size)
                        .and_then(|size| usize::try_from(size).ok())
                        .ok_or_else(|| {
                            format!("max_match {size:?} is not a size such as \"1KiB\"")
                        })?,
                };
                Ok(Replace {
                    regex,
                    with: replace.with.into_bytes(),
                    max_match,
                })
            })
            .collect::<Result<Vec<_>, String>>()
            .map_err(in_block)?;

        if targets.is_empty() && replaces.is_empty() {
            return Err(in_block(
                "nothing to change: add `json` or `replace`".to_string(),
            ));
        }
        Ok(Self { targets, replaces })
    }

    /// This block, among the blocks keeping something for a request
    fn id(&self) -> usize {
        std::ptr::from_ref(self) as usize
    }
}

/// A path, compiled
fn target(path: &str, action: Action) -> Result<Target, String> {
    let steps = path::parse(path)?;
    let key = match (&action, steps.last()) {
        (Action::Set(_), Some(Step::Key(key))) => {
            Some(serde_json::to_vec(key).map_err(|err| err.to_string())?)
        }
        _ => None,
    };
    Ok(Target { steps, action, key })
}

/// `application/json`, or a type ending with `+json` (`application/merge-patch+json`)
fn is_json(request: &RequestHeader) -> bool {
    let media_type = request
        .headers
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.split(';').next())
        .map(str::trim)
        .unwrap_or_default();
    media_type.eq_ignore_ascii_case("application/json")
        || media_type
            .get(media_type.len().saturating_sub(5)..)
            .is_some_and(|suffix| suffix.eq_ignore_ascii_case("+json"))
}

/// Whether the request has a body (a length that isn't 0, or a chunked one)
fn has_body(request: &RequestHeader) -> bool {
    let length = request.headers.get(header::CONTENT_LENGTH);
    length.is_some_and(|length| length.as_bytes() != b"0")
        || request.headers.contains_key(header::TRANSFER_ENCODING)
}

#[async_trait]
impl Middleware for BodyRewrite {
    /// Decides whether the body is changed, and sends the request without its length if so.
    /// Runs for each upstream tried: a retried body starts again.
    async fn upstream_request(
        &self,
        _session: &mut Session,
        request: &mut RequestHeader,
        ctx: &mut dyn RequestCtx,
    ) -> Result<()> {
        let json = !self.targets.is_empty() && is_json(request);
        if !has_body(request) || (!json && self.replaces.is_empty()) {
            return Ok(());
        }
        request.remove_header(&header::CONTENT_LENGTH);
        // HTTP/2 frames the body itself; Pingora removed `Transfer-Encoding` for it
        if request.version != Version::HTTP_2 {
            request.insert_header(header::TRANSFER_ENCODING, "chunked")?;
        }
        let rewriting = Rewriting {
            json: json.then(Editor::default),
            replacing: self.replaces.iter().map(|_| Replacing::default()).collect(),
        };
        set_block_state(ctx, self.id(), rewriting);
        Ok(())
    }

    fn request_body(
        &self,
        body: &mut Option<Bytes>,
        end_of_stream: bool,
        ctx: &mut dyn RequestCtx,
    ) -> pingora::Result<()> {
        let Some(rewriting) = block_state::<Rewriting>(ctx, self.id()) else {
            return Ok(());
        };
        let mut part = body.take().unwrap_or_default();
        if let Some(editor) = &mut rewriting.json {
            part = editor.edit(&self.targets, &part, end_of_stream);
        }
        for (replace, replacing) in self.replaces.iter().zip(&mut rewriting.replacing) {
            part = replacing.replace(replace, &part, end_of_stream);
        }
        *body = Some(part);
        Ok(())
    }
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

    #[test]
    fn test_settings() {
        let edits = block(serde_json::json!({
            "json": { "set": { "$.role": "guest" }, "remove": ["$.debug"] },
            "replace": { "pattern": "secret-[0-9]+", "with": "[redacted]", "max_match": "64B" },
        }))
        .unwrap();
        assert_eq!((edits.targets.len(), edits.replaces.len()), (2, 1));
        assert_eq!(edits.replaces[0].max_match, 64);

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
                serde_json::json!({ "replace": { "pattern": "a", "with": "b", "max_match": "big" } }),
                "body_rewrite: max_match \"big\" is not a size such as \"1KiB\"",
            ),
        ] {
            assert_eq!(block(config).err().as_deref(), Some(err));
        }
        let bad = block(serde_json::json!({ "replace": [{ "pattern": "(", "with": "" }] })).err();
        assert!(bad.is_some_and(|err| err.starts_with("body_rewrite: pattern \"(\":")));
    }
}
