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

/// The request's media type (`application/json` in `application/json; charset=utf-8`), lower
/// case
fn media_type(request: &RequestHeader) -> String {
    request
        .headers
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.split(';').next())
        .map(|media_type| media_type.trim().to_ascii_lowercase())
        .unwrap_or_default()
}

/// `application/json`, or a type ending with `+json` (`application/merge-patch+json`)
fn is_json(media_type: &str) -> bool {
    media_type == "application/json" || media_type.ends_with("+json")
}

/// Text that regular expressions can be replaced in: not a file, an image, or a type not given
fn is_text(media_type: &str) -> bool {
    media_type.starts_with("text/")
        || is_json(media_type)
        || media_type.ends_with("+xml")
        || matches!(
            media_type,
            "application/xml"
                | "application/x-www-form-urlencoded"
                | "application/javascript"
                | "application/graphql"
                | "application/x-ndjson"
        )
}

/// Compressed (`Content-Encoding` other than `identity`): bytes no change can be made in
fn is_encoded(request: &RequestHeader) -> bool {
    request
        .headers
        .get(header::CONTENT_ENCODING)
        .is_some_and(|encoding| !encoding.as_bytes().eq_ignore_ascii_case(b"identity"))
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
        if !has_body(request) || is_encoded(request) {
            return Ok(());
        }
        let media_type = media_type(request);
        let json = !self.targets.is_empty() && is_json(&media_type);
        let replace = !self.replaces.is_empty() && is_text(&media_type);
        if !json && !replace {
            return Ok(());
        }
        request.remove_header(&header::CONTENT_LENGTH);
        // HTTP/2 frames the body itself; Pingora removed `Transfer-Encoding` for it
        if request.version != Version::HTTP_2 {
            request.insert_header(header::TRANSFER_ENCODING, "chunked")?;
        }
        let rewriting = Rewriting {
            json: json.then(Editor::default),
            replacing: if replace {
                self.replaces.iter().map(|_| Replacing::default()).collect()
            } else {
                vec![]
            },
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
            let edited = editor.edit(&self.targets, &part, end_of_stream);
            part = edited.ok_or_else(|| {
                pingora::Error::explain(
                    pingora::ErrorType::HTTPStatus(413),
                    "body_rewrite: a key of the body is too long to change it",
                )
            })?;
        }
        // Empty when the body isn't text
        for (replace, replacing) in self.replaces.iter().zip(&mut rewriting.replacing) {
            part = replacing.replace(replace, &part, end_of_stream);
        }
        *body = Some(part);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use pingora::http::RequestHeader;

    use crate::RoutePlugin;

    use super::{is_encoded, is_json, is_text, media_type, BodyRewrite};

    #[test]
    fn test_which_bodies_are_changed() {
        let request = |headers: &[(&'static str, &str)]| {
            let mut request = RequestHeader::build("POST", b"/", None).unwrap();
            for (name, value) in headers {
                request.insert_header(*name, *value).unwrap();
            }
            request
        };
        let typed = |value: &str| media_type(&request(&[("content-type", value)]));

        assert_eq!(typed("Application/JSON; charset=utf-8"), "application/json");
        assert!(is_json(&typed("application/merge-patch+json")));
        assert!(!is_json(&typed("text/plain")));
        for text in [
            "text/plain",
            "text/csv",
            "application/json",
            "application/soap+xml",
            "application/x-www-form-urlencoded",
        ] {
            assert!(is_text(&typed(text)), "{text}");
        }
        for binary in [
            "image/png",
            "application/octet-stream",
            "multipart/form-data; boundary=x",
            "application/pdf",
        ] {
            assert!(!is_text(&typed(binary)), "{binary}");
        }
        assert!(!is_text(&media_type(&request(&[]))));

        assert!(is_encoded(&request(&[("content-encoding", "gzip")])));
        assert!(!is_encoded(&request(&[("content-encoding", "identity")])));
        assert!(!is_encoded(&request(&[])));
    }

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
