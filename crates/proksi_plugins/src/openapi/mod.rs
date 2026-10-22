//! `openapi { spec = "/etc/proksi/api.json" }`: only lets through requests in an API's
//! contract, as an `OpenAPI` 3 spec (JSON or YAML) says: a path of the spec (`404` otherwise), one
//! of its methods (`405`, with `Allow`), and its required query and header parameters (`400`).
//! Bodies aren't checked.
//!
//! Specs are loaded and kept fresh in the background (`source`), compiled to what's checked
//! (`spec`, `contract`): a request is one lookup in a path tree, a method, and a few lookups of
//! its parameters.

use std::{sync::Arc, time::Duration};

use anyhow::Result;
use async_trait::async_trait;
use http::{header, StatusCode};
use pingora::{http::ResponseHeader, proxy::Session};

use crate::{
    rate_limit::parse_duration, respond::respond_with_body, settings::OnMissingSpec, RequestCtx,
    RoutePlugin,
};

use super::Middleware;

use contract::Refusal;
use source::{Origin, Source};

mod contract;
mod source;
mod spec;

pub use source::set_cache_dir;

const DEFAULT_REFRESH: Duration = Duration::from_secs(60);

pub(crate) struct OpenApi {
    source: Arc<Source>,
    /// Without a trailing `/`
    base_path: Option<String>,
    on_missing_spec: OnMissingSpec,
}

impl OpenApi {
    pub(crate) fn new(plugin: &RoutePlugin) -> Result<Self, String> {
        let settings: super::settings::OpenApi = super::settings::read("openapi", plugin)?;
        let origin = match (settings.spec, settings.url) {
            (Some(file), None) => Origin::File(file.into()),
            (None, Some(url)) if url.starts_with("http://") || url.starts_with("https://") => {
                Origin::Url(url)
            }
            (None, Some(url)) => return Err(format!("openapi: url {url:?} isn't an http(s) URL")),
            _ => return Err("openapi: give either `spec` (a file) or `url`".to_string()),
        };
        let refresh = match &settings.refresh {
            None => DEFAULT_REFRESH,
            Some(every) => parse_duration(every).ok_or_else(|| {
                format!("openapi: refresh {every:?} is not a duration such as \"30s\" or \"5m\"")
            })?,
        };
        let base_path = match settings.base_path {
            None => None,
            Some(base) if base.starts_with('/') => Some(base.trim_end_matches('/').to_string()),
            Some(base) => return Err(format!("openapi: base_path {base:?} must start with `/`")),
        };
        Ok(Self {
            source: Source::of(origin, refresh),
            base_path: base_path.filter(|base| !base.is_empty()),
            on_missing_spec: settings.on_missing_spec.unwrap_or(OnMissingSpec::Allow),
        })
    }
}

#[async_trait]
impl Middleware for OpenApi {
    async fn request(&self, session: &mut Session, ctx: &mut dyn RequestCtx) -> Result<bool> {
        let contract = self.source.contract();
        let Some(contract) = contract.as_ref() else {
            return match self.on_missing_spec {
                OnMissingSpec::Allow => Ok(false),
                OnMissingSpec::Deny => {
                    refuse(
                        session,
                        ctx,
                        StatusCode::SERVICE_UNAVAILABLE,
                        None,
                        "the API's contract isn't loaded yet",
                    )
                    .await
                }
            };
        };
        let request = session.req_header();
        let path = ctx.normalize_path(request.uri.path());
        let path = match &self.base_path {
            None => Some(path.as_ref()),
            Some(base) => path
                .strip_prefix(base.as_str())
                .filter(|rest| rest.starts_with('/')),
        };
        let checked = match path {
            Some(path) => {
                contract.check(&request.method, path, request.uri.query(), &request.headers)
            }
            None => Err(Refusal::NotFound),
        };
        match checked {
            Ok(()) => Ok(false),
            Err(Refusal::NotFound) => {
                refuse(
                    session,
                    ctx,
                    StatusCode::NOT_FOUND,
                    None,
                    "not a path of the API",
                )
                .await
            }
            Err(Refusal::Method(allow)) => {
                refuse(
                    session,
                    ctx,
                    StatusCode::METHOD_NOT_ALLOWED,
                    Some(allow),
                    "not a method of this path of the API",
                )
                .await
            }
            Err(Refusal::Missing(place, name)) => {
                let why = format!("the required {place} `{name}` is missing");
                refuse(session, ctx, StatusCode::BAD_REQUEST, None, &why).await
            }
        }
    }
}

async fn refuse(
    session: &mut Session,
    ctx: &mut dyn RequestCtx,
    status: StatusCode,
    allow: Option<String>,
    why: &str,
) -> Result<bool> {
    let mut response = ResponseHeader::build_no_case(status, Some(3))?;
    response.insert_header(header::CONTENT_TYPE, "text/plain; charset=utf-8")?;
    if let Some(allow) = allow {
        response.insert_header(header::ALLOW, allow)?;
    }
    respond_with_body(session, ctx, response, format!("{why}\n").into()).await?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use crate::RoutePlugin;

    use super::OpenApi;

    fn block(config: serde_json::Value) -> Result<OpenApi, String> {
        OpenApi::new(&RoutePlugin {
            name: "openapi".into(),
            config: serde_json::from_value(config).unwrap(),
        })
    }

    #[test]
    fn test_settings() {
        let api =
            block(serde_json::json!({ "spec": "/nonexistent/api.json", "base_path": "/api/" }))
                .unwrap();
        assert_eq!(api.base_path.as_deref(), Some("/api"));
        for (config, err) in [
            (
                serde_json::json!({}),
                "openapi: give either `spec` (a file) or `url`",
            ),
            (
                serde_json::json!({ "spec": "a.json", "url": "https://a.test/a.json" }),
                "openapi: give either `spec` (a file) or `url`",
            ),
            (
                serde_json::json!({ "url": "ftp://a.test/a.json" }),
                "openapi: url \"ftp://a.test/a.json\" isn't an http(s) URL",
            ),
            (
                serde_json::json!({ "spec": "a.json", "base_path": "api" }),
                "openapi: base_path \"api\" must start with `/`",
            ),
            (
                serde_json::json!({ "spec": "a.json", "refresh": "often" }),
                "openapi: refresh \"often\" is not a duration such as \"30s\" or \"5m\"",
            ),
        ] {
            assert_eq!(block(config).err().as_deref(), Some(err));
        }
    }
}
