use async_trait::async_trait;
use http::{
    header::{CONTENT_LENGTH, CONTENT_TYPE, LOCATION},
    uri::Scheme,
    StatusCode, Uri,
};
use pingora::http::ResponseHeader;
use pingora::upstreams::peer::HttpPeer;

use pingora::proxy::{ProxyHttp, Session};
use tracing::info;

use crate::stores::global;

pub struct HttpLB {}

#[async_trait]
impl ProxyHttp for HttpLB {
    type CTX = ();

    fn new_ctx(&self) -> Self::CTX {}

    /// Filters based on path (used by LetsEncrypt/ZeroSSL challenges)
    async fn request_filter(
        &self,
        session: &mut Session,
        _ctx: &mut Self::CTX,
    ) -> pingora::Result<bool> {
        let req_header = session.req_header();
        let current_uri = &req_header.uri;

        let host = get_host(session);
        if host.is_empty() {
            return Err(pingora::Error::new(pingora::ErrorType::HTTPStatus(400)));
        }

        if current_uri.path() == "/ping" {
            let sample_body = bytes::Bytes::from("pong");
            let mut res_headers = ResponseHeader::build_no_case(StatusCode::OK, None)?;
            res_headers.append_header(CONTENT_TYPE, "text/plain")?;
            res_headers.append_header(CONTENT_LENGTH, sample_body.len())?;

            session
                .write_response_header(Box::new(res_headers), false)
                .await?;
            session.write_response_body(Some(sample_body), true).await?;
            return Ok(true);
        }

        // LetsEncrypt/ZeroSSL challenge
        if current_uri
            .path()
            .starts_with("/.well-known/acme-challenge")
        {
            let challenge_from_host = global::get_store().get_challenge(host).await;

            if challenge_from_host.is_none() {
                return Err(pingora::Error::new(pingora::ErrorType::HTTPStatus(404)));
            }

            let challenge_from_host = challenge_from_host.unwrap();

            // Get the token and proof from the challenge store
            let (token, proof) = challenge_from_host;
            // Get the token from the URL
            let token_from_url = current_uri.path().split('/').last().unwrap();

            // Token is not the same as the one provided
            if token != token_from_url {
                return Err(pingora::Error::new(pingora::ErrorType::HTTPStatus(404)));
            }

            let sample_body = bytes::Bytes::from(proof.clone());
            let mut res_headers = ResponseHeader::build_no_case(StatusCode::OK, Some(2))?;
            res_headers.append_header(CONTENT_TYPE, "text/plain")?;
            res_headers.append_header(CONTENT_LENGTH, sample_body.len())?;

            session
                .write_response_header(Box::new(res_headers), false)
                .await?;
            session.write_response_body(Some(sample_body), true).await?;

            return Ok(true);
        }

        // Redirect to https
        let new_uri = Uri::builder()
            .scheme(Scheme::HTTPS)
            .authority(host)
            .path_and_query(current_uri.path_and_query().unwrap().to_owned())
            .build()
            .unwrap();

        let mut res_headers =
            ResponseHeader::build_no_case(StatusCode::PERMANENT_REDIRECT, Some(1))?;

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

        return Ok(true);
    }

    /// In the case of port 80, we won't have an upstream to choose from.
    /// We will use request filters to handle LetsEncrypt/ZeroSSL challenges.
    ///
    async fn upstream_peer(
        &self,
        _session: &mut Session,
        _ctx: &mut Self::CTX,
    ) -> pingora::Result<Box<HttpPeer>> {
        Err(pingora::Error::new(pingora::ErrorType::HTTPStatus(404)))
    }
}

/// Retrieves the host from the request headers based on
/// whether the request is HTTP/1.1 or HTTP/2
fn get_host(session: &Session) -> &str {
    if let Some(host) = session.get_header(http::header::HOST) {
        return host.to_str().unwrap_or("");
    }

    if let Some(host) = session.req_header().uri.host() {
        return host;
    }

    ""
}
