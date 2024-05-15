use std::{fs::File, io::Read, sync::Arc};

use async_trait::async_trait;
use http::{
    header::{CONTENT_LENGTH, CONTENT_TYPE, LOCATION},
    uri::Scheme,
    StatusCode, Uri,
};
use pingora::upstreams::peer::HttpPeer;
use pingora_http::ResponseHeader;
use pingora_load_balancing::{selection::RoundRobin, LoadBalancer};
use pingora_proxy::{ProxyHttp, Session};
use tracing::info;

pub struct HttpLB(pub Arc<LoadBalancer<RoundRobin>>);

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
        // LetsEncrypt/ZeroSSL challenge
        let req_header = session.req_header();
        let current_uri = req_header.uri.clone();

        let host = get_host(session);
        if host.is_empty() {
            return Err(pingora::Error::new(pingora::ErrorType::HTTPStatus(400)));
        }

        if current_uri.path() == "/ping" {
            info!("Ping request received");
            let sample_body = bytes::Bytes::from("pong");
            let mut res_headers = ResponseHeader::build_no_case(StatusCode::OK, Some(2))?;
            res_headers.append_header(CONTENT_TYPE, "text/plain")?;
            res_headers.append_header(CONTENT_LENGTH, sample_body.len())?;

            session.write_response_header(Box::new(res_headers)).await?;
            session.write_response_body(sample_body).await?;
            return Ok(true);
        }

        if current_uri
            .path()
            .starts_with("/.well-known/acme-challenge")
        {
            let challenge_path = format!("./data/challenges/{}", &host);
            let folder = std::path::Path::new(&challenge_path);
            if !folder.is_dir() {
                // Challenge doesn't exist on disk
                return Err(pingora::Error::new(pingora::ErrorType::HTTPStatus(404)));
            }

            // open challenge
            let mut challenge_data = String::from("");
            let challenge = File::open(format!("{}/meta.csv", challenge_path));
            if let Ok(mut challenge) = challenge {
                challenge.read_to_string(&mut challenge_data).unwrap();
            }

            // Nothing to read;
            if challenge_data.is_empty() {
                return Err(pingora::Error::new(pingora::ErrorType::HTTPStatus(404)));
            }

            let token_from_file = challenge_data.split(';').collect::<Vec<&str>>();
            let token_from_url = current_uri.path().split('/').last().unwrap();

            // Weird token being provided
            if token_from_file[token_from_file.len() - 1] != token_from_url {
                return Err(pingora::Error::new(pingora::ErrorType::HTTPStatus(404)));
            }

            let key_auth = token_from_file[1];

            let sample_body = bytes::Bytes::from(key_auth.to_string());
            let mut res_headers = ResponseHeader::build_no_case(StatusCode::OK, Some(2))?;
            res_headers.append_header(CONTENT_TYPE, "text/plain")?;
            res_headers.append_header(CONTENT_LENGTH, sample_body.len())?;

            session.write_response_header(Box::new(res_headers)).await?;
            session.write_response_body(sample_body).await?;

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

        println!("redirecting to https {new_uri:?}");
        res_headers.append_header(LOCATION, new_uri.to_string())?;
        res_headers.append_header(CONTENT_TYPE, "text/plain")?;
        res_headers.append_header(CONTENT_LENGTH, 0)?;

        session.write_response_header(Box::new(res_headers)).await?;
        session
            .write_response_body(bytes::Bytes::from_static(b""))
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

/// Retrieves the host from the request headers based on whether
/// the request is HTTP/1.1 or HTTP/2
fn get_host(session: &mut Session) -> String {
    if let Some(host) = session.get_header(http::header::HOST) {
        if let Ok(host_str) = host.to_str() {
            return host_str.to_string();
        }
    }

    if let Some(host) = session.req_header().uri.host() {
        return host.to_string();
    }

    "".to_string()
}
