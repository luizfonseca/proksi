//! Requests on the HTTP port that aren't proxied: health checks (`/ping`),
//! ACME challenges, and redirects to HTTPS

use http::{
    header::{CONTENT_LENGTH, CONTENT_TYPE, LOCATION},
    uri::Scheme,
    StatusCode, Uri,
};
use pingora::{http::ResponseHeader, proxy::Session};
use tracing::info;

use crate::stores::global;

/// Answers `/ping` with `pong`. Returns `true` if the request was handled.
pub async fn serve_ping(session: &mut Session) -> pingora::Result<bool> {
    if session.req_header().uri.path() != "/ping" {
        return Ok(false);
    }

    respond_text(session, StatusCode::OK, "pong").await?;
    Ok(true)
}

/// Answers Let's Encrypt/ZeroSSL HTTP-01 challenges for `host`.
/// Returns `true` if the request was handled.
pub async fn serve_acme_challenge(session: &mut Session, host: &str) -> pingora::Result<bool> {
    let path = session.req_header().uri.path().to_string();
    if !path.starts_with("/.well-known/acme-challenge") {
        return Ok(false);
    }

    // Get the token and proof from the challenge store
    let Some((token, proof)) = global::get_store().get_challenge(host).await else {
        return Err(pingora::Error::new(pingora::ErrorType::HTTPStatus(404)));
    };

    // The token in the URL must be the one of the challenge
    if token != path.rsplit('/').next().unwrap_or_default() {
        return Err(pingora::Error::new(pingora::ErrorType::HTTPStatus(404)));
    }

    respond_text(session, StatusCode::OK, &proof).await?;
    Ok(true)
}

/// Redirects the request to the same URL over HTTPS (on `port`, when it isn't 443)
pub async fn redirect_to_https(
    session: &mut Session,
    host: &str,
    port: Option<u16>,
) -> pingora::Result<bool> {
    let authority = match port {
        Some(port) => format!("{}:{port}", host.split(':').next().unwrap_or_default()),
        None => host.to_string(),
    };
    let path_and_query = session
        .req_header()
        .uri
        .path_and_query()
        .map_or("/", http::uri::PathAndQuery::as_str)
        .to_string();
    let Ok(new_uri) = Uri::builder()
        .scheme(Scheme::HTTPS)
        .authority(authority)
        .path_and_query(path_and_query)
        .build()
    else {
        return Err(pingora::Error::new(pingora::ErrorType::HTTPStatus(400)));
    };

    let mut res_headers = ResponseHeader::build_no_case(StatusCode::PERMANENT_REDIRECT, Some(1))?;

    info!("redirecting to https {new_uri:?}");
    res_headers.append_header(LOCATION, new_uri.to_string())?;
    res_headers.append_header(CONTENT_TYPE, "text/plain")?;
    res_headers.append_header(CONTENT_LENGTH, 0)?;

    session
        .write_response_header(Box::new(res_headers), false)
        .await?;
    session
        .write_response_body(Some(bytes::Bytes::from_static(b"")), true)
        .await?;

    Ok(true)
}

async fn respond_text(
    session: &mut Session,
    status: StatusCode,
    text: &str,
) -> pingora::Result<()> {
    let body = bytes::Bytes::from(text.to_string());
    let mut res_headers = ResponseHeader::build_no_case(status, Some(2))?;
    res_headers.append_header(CONTENT_TYPE, "text/plain")?;
    res_headers.append_header(CONTENT_LENGTH, body.len())?;

    session
        .write_response_header(Box::new(res_headers), false)
        .await?;
    session.write_response_body(Some(body), true).await
}
