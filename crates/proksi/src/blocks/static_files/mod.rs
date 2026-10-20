//! `static_files { directory = "..." }`: answers a location's requests with the files of a
//! directory, by their path below the location

mod http;
mod resolve;

use std::path::PathBuf;

use anyhow::Result;
use async_trait::async_trait;
use pingora::{http::ResponseHeader, proxy::Session};

use crate::{
    config::RoutePlugin,
    proxy_server::{
        https_proxy::RouterContext,
        respond::{respond, respond_streaming},
    },
};

use super::{settings, Middleware};
use resolve::Found;

pub(super) struct StaticFiles {
    /// Canonical: what a file's canonical path must start with
    directory: PathBuf,
    /// The location's path, without its trailing `/*` (`/assets`, or empty for `/`)
    prefix: String,
    index: Vec<String>,
    /// Segments of the file served for paths with none
    fallback: Option<Vec<String>>,
    cache_control: Option<String>,
}

impl StaticFiles {
    pub(super) fn new(plugin: &RoutePlugin, location: &str) -> Result<Self, String> {
        let settings: settings::StaticFiles = settings::read("static_files", plugin)?;
        let directory = PathBuf::from(&settings.directory);
        if !directory.is_absolute() {
            return Err("static_files: directory must be an absolute path".to_string());
        }
        let directory = directory
            .canonicalize()
            .map_err(|err| format!("static_files: directory \"{}\": {err}", settings.directory))?;
        if !directory.is_dir() {
            return Err(format!(
                "static_files: \"{}\" is not a directory",
                settings.directory
            ));
        }

        let prefix = location.trim_end_matches('*').trim_end_matches('/');
        if prefix.contains([':', '*', '+']) {
            return Err(
                "static_files: the location's path can't have parameters or wildcards".to_string(),
            );
        }

        let index = settings
            .index
            .unwrap_or_else(|| vec!["index.html".to_string()]);
        if index
            .iter()
            .any(|name| resolve::segments(name).is_none_or(|segments| segments.len() != 1))
        {
            return Err("static_files: index must be file names".to_string());
        }
        let fallback = match settings.fallback {
            Some(fallback) => Some(
                resolve::segments(&fallback)
                    .filter(|segments| !segments.is_empty())
                    .ok_or("static_files: fallback must be a file's path in the directory")?,
            ),
            None => None,
        };

        Ok(Self {
            directory,
            prefix: prefix.to_string(),
            index,
            fallback,
            cache_control: settings.cache_control,
        })
    }

    /// The file for `path` (the request's, normalized), or the fallback. Asked on the request's
    /// thread, as nginx does: files served are mostly in the page cache, where these calls take
    /// microseconds, and a hop to a blocking thread for each costs far more.
    fn find(&self, path: &str) -> Option<PathBuf> {
        let below = path.strip_prefix(self.prefix.as_str()).unwrap_or(path);
        let segments = resolve::segments(below)?;
        match resolve::find(&self.directory, &segments, &self.index) {
            Found::File(file) => Some(file),
            Found::Missing => {
                match resolve::find(&self.directory, self.fallback.as_deref()?, &[]) {
                    Found::File(file) => Some(file),
                    Found::Missing => None,
                }
            }
        }
    }
}

/// An answer with no body
fn status(code: u16) -> Result<ResponseHeader> {
    let mut response = ResponseHeader::build(code, Some(4))?;
    response.insert_header(::http::header::CONTENT_LENGTH, "0")?;
    Ok(response)
}

#[async_trait]
impl Middleware for StaticFiles {
    async fn request(&self, session: &mut Session, ctx: &mut RouterContext) -> Result<bool> {
        use ::http::{header, Method};

        let method = session.req_header().method.clone();
        if method != Method::GET && method != Method::HEAD {
            let mut response = status(405)?;
            response.insert_header(header::ALLOW, "GET, HEAD")?;
            respond(session, ctx, response).await?;
            return Ok(true);
        }

        let path = crate::routing::paths::normalize_path(session.req_header().uri.path());
        let Some(path) = self.find(&path) else {
            respond(session, ctx, status(404)?).await?;
            return Ok(true);
        };
        let file = match std::fs::File::open(&path) {
            Ok(file) => file,
            Err(err) => {
                tracing::warn!("static_files: can't open {path:?}: {err}");
                respond(session, ctx, status(404)?).await?;
                return Ok(true);
            }
        };
        let metadata = file.metadata()?;
        let len = metadata.len();
        let validators = http::Validators::new(len, metadata.modified()?);

        let headers = &session.req_header().headers;
        let text =
            |name: header::HeaderName| headers.get(name).and_then(|value| value.to_str().ok());
        let not_modified =
            validators.not_modified(text(header::IF_NONE_MATCH), text(header::IF_MODIFIED_SINCE));
        let part = http::part(text(header::RANGE), len);

        let validated = |mut response: ResponseHeader| -> Result<ResponseHeader> {
            response.insert_header(header::ETAG, validators.etag.as_str())?;
            response.insert_header(header::LAST_MODIFIED, validators.last_modified.as_str())?;
            if let Some(cache_control) = &self.cache_control {
                response.insert_header(header::CACHE_CONTROL, cache_control.as_str())?;
            }
            Ok(response)
        };

        if not_modified {
            respond(
                session,
                ctx,
                validated(ResponseHeader::build(304, Some(4))?)?,
            )
            .await?;
            return Ok(true);
        }
        let (code, start, end) = match part {
            http::Part::Whole => (200, 0, len.saturating_sub(1)),
            http::Part::Range { start, end } => (206, start, end),
            http::Part::Unsatisfiable => {
                let mut response = status(416)?;
                response.insert_header(header::CONTENT_RANGE, format!("bytes */{len}"))?;
                respond(session, ctx, response).await?;
                return Ok(true);
            }
        };
        let length = if len == 0 { 0 } else { end - start + 1 };

        let mut response = validated(ResponseHeader::build(code, Some(8))?)?;
        response.insert_header(header::CONTENT_TYPE, http::content_type(&path))?;
        response.insert_header(header::CONTENT_LENGTH, length)?;
        response.insert_header(header::ACCEPT_RANGES, "bytes")?;
        if code == 206 {
            response.insert_header(header::CONTENT_RANGE, format!("bytes {start}-{end}/{len}"))?;
        }

        if method == Method::HEAD {
            respond(session, ctx, response).await?;
            return Ok(true);
        }
        let mut file = file;
        if start > 0 {
            use std::io::Seek;
            file.seek(std::io::SeekFrom::Start(start))?;
        }
        respond_streaming(session, ctx, response, file, length).await?;
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use std::borrow::Cow;

    use crate::config::RoutePlugin;

    use super::StaticFiles;

    fn static_files(config: serde_json::Value, location: &str) -> Result<StaticFiles, String> {
        StaticFiles::new(
            &RoutePlugin {
                name: Cow::Borrowed("static_files"),
                config: serde_json::from_value(config).unwrap(),
            },
            location,
        )
    }

    #[test]
    fn test_settings() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().to_string_lossy().to_string();

        let files = static_files(serde_json::json!({ "directory": path }), "/assets/*").unwrap();
        assert_eq!(files.prefix, "/assets");
        assert_eq!(files.index, vec!["index.html".to_string()]);
        assert_eq!(
            static_files(serde_json::json!({ "directory": path }), "/")
                .unwrap()
                .prefix,
            ""
        );

        let error =
            |config: serde_json::Value, location: &str| static_files(config, location).err();
        assert_eq!(
            error(serde_json::json!({ "directory": "relative/dir" }), "/"),
            Some("static_files: directory must be an absolute path".to_string())
        );
        assert!(error(
            serde_json::json!({ "directory": "/no/such/directory" }),
            "/"
        )
        .is_some());
        assert_eq!(
            error(serde_json::json!({ "directory": path }), "/users/:id"),
            Some(
                "static_files: the location's path can't have parameters or wildcards".to_string()
            )
        );
        assert_eq!(
            error(
                serde_json::json!({ "directory": path, "fallback": "/../index.html" }),
                "/"
            ),
            Some("static_files: fallback must be a file's path in the directory".to_string())
        );
        assert_eq!(
            error(
                serde_json::json!({ "directory": path, "index": ["a/index.html"] }),
                "/"
            ),
            Some("static_files: index must be file names".to_string())
        );
        assert!(error(serde_json::json!({ "directory": path, "root": "/x" }), "/").is_some());
    }
}
