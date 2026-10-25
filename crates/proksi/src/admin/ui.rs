//! The admin UI (ADR 0008): its files, embedded by `proksi-admin-ui`, served by the admin
//! listener at every path that isn't the API's

use bytes::Bytes;
use http::{header, Method, StatusCode};
use pingora::{http::ResponseHeader, protocols::http::ServerSession};
use proksi_admin_ui::Asset;

use super::{Answer, Reply};

/// Nothing but the UI's own files (and GitHub's releases, for the "new version" notice), and no
/// framing
const CONTENT_SECURITY_POLICY: &str = "default-src 'self'; connect-src 'self' https://api.github.com; \
     frame-ancestors 'none'; base-uri 'none'; form-action 'self'";

/// How a file is sent
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Encoding {
    Brotli,
    Gzip,
    Identity,
}

/// Whether `path` is the UI's (anything but the API and metrics)
pub fn serves(path: &str) -> bool {
    !path.starts_with("/api/") && path != "/metrics"
}

/// The file at `path` (the UI's page for its own paths), compressed as the client accepts
pub fn answer(method: &Method, path: &str, accept_encoding: Option<&str>) -> Answer {
    if method != Method::GET && method != Method::HEAD {
        return Answer::Reply(Reply::error(
            StatusCode::METHOD_NOT_ALLOWED,
            "method not allowed",
        ));
    }
    let Some(asset) = proksi_admin_ui::get(path) else {
        return Answer::Reply(Reply::error(StatusCode::NOT_FOUND, "not found"));
    };
    let accepts = |name: &str| {
        accept_encoding.is_some_and(|value| {
            value
                .split(',')
                .any(|encoding| encoding.split(';').next().unwrap_or_default().trim() == name)
        })
    };
    let encoding = if asset.brotli.is_some() && accepts("br") {
        Encoding::Brotli
    } else if asset.gzip.is_some() && accepts("gzip") {
        Encoding::Gzip
    } else {
        Encoding::Identity
    };
    Answer::Ui(asset, encoding)
}

/// The response's headers
fn response_header(asset: &Asset, encoding: Encoding, length: usize) -> Option<ResponseHeader> {
    let mut header = ResponseHeader::build(StatusCode::OK, None).ok()?;
    header
        .insert_header(header::CONTENT_TYPE, asset.content_type)
        .ok()?;
    header.insert_header(header::CONTENT_LENGTH, length).ok()?;
    header.insert_header(header::VARY, "accept-encoding").ok()?;
    match encoding {
        Encoding::Brotli => header.insert_header(header::CONTENT_ENCODING, "br").ok()?,
        Encoding::Gzip => header
            .insert_header(header::CONTENT_ENCODING, "gzip")
            .ok()?,
        Encoding::Identity => {}
    }
    // Hashed files never change; pages are checked again, to get a new version's files
    let cache = if asset.immutable() {
        "public, max-age=31536000, immutable"
    } else {
        "no-cache"
    };
    header.insert_header(header::CACHE_CONTROL, cache).ok()?;
    header
        .insert_header(header::CONTENT_SECURITY_POLICY, CONTENT_SECURITY_POLICY)
        .ok()?;
    header
        .insert_header(header::X_CONTENT_TYPE_OPTIONS, "nosniff")
        .ok()?;
    header
        .insert_header(header::REFERRER_POLICY, "no-referrer")
        .ok()?;
    Some(header)
}

/// Writes a file; returns whether it was written
pub async fn write(
    session: &mut ServerSession,
    asset: &'static Asset,
    encoding: Encoding,
    head: bool,
) -> bool {
    let body = match encoding {
        Encoding::Brotli => asset.brotli.unwrap_or(asset.body),
        Encoding::Gzip => asset.gzip.unwrap_or(asset.body),
        Encoding::Identity => asset.body,
    };
    let Some(header) = response_header(asset, encoding, body.len()) else {
        return false;
    };
    if session
        .write_response_header(Box::new(header))
        .await
        .is_err()
    {
        return false;
    }
    let body = if head {
        Bytes::new()
    } else {
        Bytes::from_static(body)
    };
    session.write_response_body(body, true).await.is_ok()
}

#[cfg(test)]
mod tests {
    use http::Method;

    use super::{answer, serves, Encoding, CONTENT_SECURITY_POLICY};
    use crate::admin::Answer;

    #[test]
    fn test_pages_only_reach_their_own_origin_and_github_releases() {
        let directives: Vec<&str> = CONTENT_SECURITY_POLICY.split("; ").collect();
        assert!(directives.contains(&"default-src 'self'"));
        // The "new version" notice reads GitHub's releases
        assert!(directives.contains(&"connect-src 'self' https://api.github.com"));
    }

    #[test]
    fn test_the_ui_has_every_path_but_the_api() {
        assert!(serves("/"));
        assert!(serves("/hosts/example.com"));
        assert!(!serves("/api/v1/hosts"));
        assert!(!serves("/metrics"));
    }

    #[test]
    fn test_pages() {
        let Answer::Ui(asset, encoding) = answer(&Method::GET, "/hosts", Some("gzip, br")) else {
            panic!("a file");
        };
        assert_eq!(asset.path, "/index.html");
        assert_eq!(
            encoding,
            if asset.brotli.is_some() {
                Encoding::Brotli
            } else {
                Encoding::Identity
            }
        );
        assert!(matches!(
            answer(&Method::POST, "/hosts", None),
            Answer::Reply(reply) if reply.status == http::StatusCode::METHOD_NOT_ALLOWED
        ));
    }
}
