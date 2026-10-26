//! `request_id { suffix = "ams1" }`: gives each request an ID, sent to the upstream and back to
//! the client as `X-Request-Id` (or `header_name`), and logged. An ID sent by a trusted proxy is
//! kept, so a request has one ID across the proxies it goes through.

use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::Result;
use async_trait::async_trait;
use bytes::BytesMut;
use http::{HeaderName, HeaderValue};
use pingora::{
    http::{RequestHeader, ResponseHeader},
    proxy::Session,
};
use rand::RngCore;
use uuid::Uuid;

use crate::{RequestCtx, RoutePlugin};

use super::Middleware;

const HEADER: &str = "X-Request-Id";

/// The longest ID kept from a trusted proxy
const MAX_KEPT: usize = 200;

/// The longest suffix
const MAX_SUFFIX: usize = 32;

/// A UUID's length, as text
const UUID: usize = 36;

pub(super) struct RequestId {
    /// The header's name, as written (Pingora parses it once more when it's set)
    header: bytes::Bytes,
    /// The same, to read requests
    name: HeaderName,
    /// `-` and the suffix, or nothing
    suffix: String,
}

impl RequestId {
    pub(super) fn new(plugin: &RoutePlugin) -> Result<Self, String> {
        let settings: super::settings::RequestId = super::settings::read("request_id", plugin)?;
        let header = settings.header_name.unwrap_or_else(|| HEADER.to_string());
        let name = HeaderName::from_bytes(header.as_bytes())
            .map_err(|_| format!("request_id: header_name {header:?} isn't a valid header name"))?;
        let header = bytes::Bytes::from(header);
        let Some(suffix) = settings.suffix else {
            return Ok(Self {
                header,
                name,
                suffix: String::new(),
            });
        };
        let valid = |byte: &u8| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.');
        if suffix.is_empty() || suffix.len() > MAX_SUFFIX || !suffix.bytes().all(|b| valid(&b)) {
            let allowed = "letters, digits, `-`, `_` or `.`";
            return Err(format!(
                "request_id: suffix {suffix:?} must be 1 to {MAX_SUFFIX} {allowed}"
            ));
        }
        Ok(Self {
            header,
            name,
            suffix: format!("-{suffix}"),
        })
    }

    /// A UUID v7 (it starts with the time, so IDs sort by when requests came) and the suffix.
    /// `Uuid::now_v7` would do, but it takes a global lock for each ID.
    fn generate(&self) -> HeaderValue {
        let since_epoch = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default();
        let millis = u64::try_from(since_epoch.as_millis()).unwrap_or(u64::MAX);
        let mut random = [0u8; 10];
        rand::thread_rng().fill_bytes(&mut random);
        let uuid = uuid::Builder::from_unix_timestamp_millis(millis, &random).into_uuid();

        let mut id = BytesMut::with_capacity(UUID + self.suffix.len());
        id.extend_from_slice(
            uuid.hyphenated()
                .encode_lower(&mut Uuid::encode_buffer())
                .as_bytes(),
        );
        id.extend_from_slice(self.suffix.as_bytes());
        HeaderValue::from_maybe_shared(id.freeze()).unwrap_or_else(|_| HeaderValue::from_static(""))
    }
}

/// The ID a trusted proxy sent in `name`, if it's one we can pass on
fn kept(session: &Session, ctx: &dyn RequestCtx, name: &HeaderName) -> Option<HeaderValue> {
    let peer = session.client_addr()?.as_inet()?.ip();
    if !ctx.is_trusted(peer) {
        return None;
    }
    let id = session.req_header().headers.get(name)?;
    let printable = id
        .as_bytes()
        .iter()
        .all(|byte| (0x21..=0x7e).contains(byte));
    (printable && !id.is_empty() && id.len() <= MAX_KEPT).then(|| id.clone())
}

#[async_trait]
impl Middleware for RequestId {
    async fn request(&self, session: &mut Session, ctx: &mut dyn RequestCtx) -> Result<bool> {
        let id = kept(session, ctx, &self.name).unwrap_or_else(|| self.generate());
        ctx.set_request_id(id);
        Ok(false)
    }

    async fn upstream_request(
        &self,
        _session: &mut Session,
        request: &mut RequestHeader,
        ctx: &mut dyn RequestCtx,
    ) -> Result<()> {
        if let Some(id) = ctx.request_id() {
            request.insert_header(self.header.clone(), id.clone())?;
        }
        Ok(())
    }

    async fn response(
        &self,
        _session: &mut Session,
        response: &mut ResponseHeader,
        ctx: &mut dyn RequestCtx,
    ) -> Result<()> {
        if let Some(id) = ctx.request_id() {
            response.insert_header(self.header.clone(), id.clone())?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::borrow::Cow;

    use super::RequestId;
    use crate::RoutePlugin;

    fn request_id(config: serde_json::Value) -> Result<RequestId, String> {
        let config = config
            .as_object()
            .unwrap()
            .iter()
            .map(|(key, value)| (Cow::Owned(key.clone()), value.clone()))
            .collect();
        RequestId::new(&RoutePlugin {
            name: "request_id".into(),
            config: Some(config),
        })
    }

    #[test]
    fn test_ids_are_uuid_v7() {
        let block = request_id(serde_json::json!({})).unwrap();
        let (first, second) = (block.generate(), block.generate());
        let first = uuid::Uuid::parse_str(first.to_str().unwrap()).unwrap();
        let second = uuid::Uuid::parse_str(second.to_str().unwrap()).unwrap();
        assert_eq!(first.get_version_num(), 7);
        assert_ne!(first, second);
        // The time comes first
        let (time, _) = first.get_timestamp().unwrap().to_unix();
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        assert!(now.abs_diff(time) < 5, "{time} {now}");
    }

    #[test]
    fn test_suffix() {
        let block = request_id(serde_json::json!({ "suffix": "ams-1" })).unwrap();
        let id = block.generate();
        let id = id.to_str().unwrap();
        assert_eq!(id.len(), 36 + 6, "{id}");
        assert!(id.ends_with("-ams-1"), "{id}");
        assert!(uuid::Uuid::parse_str(&id[..36]).is_ok(), "{id}");

        let longest = "a".repeat(32);
        let id = request_id(serde_json::json!({ "suffix": longest }))
            .unwrap()
            .generate();
        assert!(id.to_str().unwrap().ends_with(&longest));
    }

    #[test]
    fn test_invalid_suffixes() {
        for suffix in ["", "a b", "ams/1", &"a".repeat(33)] {
            let err = request_id(serde_json::json!({ "suffix": suffix })).err();
            assert!(
                err.is_some_and(|err| err.starts_with("request_id: suffix")),
                "{suffix:?}"
            );
        }
    }

    #[test]
    fn test_header() {
        let block = request_id(serde_json::json!({})).unwrap();
        assert_eq!(&block.header[..], b"X-Request-Id");
        let block = request_id(serde_json::json!({ "header_name": "X-Trace-Id" })).unwrap();
        assert_eq!(
            (&block.header[..], block.name.as_str()),
            (&b"X-Trace-Id"[..], "x-trace-id")
        );
        let err = request_id(serde_json::json!({ "header_name": "x trace" })).err();
        assert_eq!(
            err.as_deref(),
            Some("request_id: header_name \"x trace\" isn't a valid header name")
        );
    }

    #[test]
    fn test_generating_allocates_once() {
        let block = request_id(serde_json::json!({ "suffix": "ams1" })).unwrap();
        block.generate();
        // The header value only
        let (count, _) = crate::allocations::allocations(|| block.generate());
        assert_eq!(count, 1);
    }
}
