//! What plugins can ask of proksi about a request ([`RequestCtx`]), from its [`RouterContext`]

use std::{borrow::Cow, net::IpAddr, sync::Arc, time::Duration};

use async_trait::async_trait;
use bytes::Bytes;
use http::{HeaderName, HeaderValue};
use pingora::{http::ResponseHeader, proxy::Session};
use proksi_plugins::RequestCtx;
use proksi_store::Store;

use crate::proxy_server::{client, https_proxy::RouterContext};

#[async_trait]
impl RequestCtx for RouterContext {
    fn host(&self) -> &str {
        &self.host
    }

    fn request_id(&self) -> Option<&HeaderValue> {
        self.request_id.as_ref()
    }

    fn set_request_id(&mut self, id: HeaderValue) {
        self.request_id = Some(id);
    }

    fn count_request_body(&mut self, bytes: u64) -> u64 {
        self.request_body_bytes += bytes;
        self.request_body_bytes
    }

    fn client(&self, session: &Session) -> Option<IpAddr> {
        client::of_session(session)
    }

    fn is_trusted(&self, peer: IpAddr) -> bool {
        client::is_trusted(peer)
    }

    fn normalize_path<'a>(&self, path: &'a str) -> Cow<'a, str> {
        crate::routing::paths::normalize_path(path)
    }

    async fn response_blocks(&mut self, session: &mut Session, response: &mut ResponseHeader) {
        if let Some(location) = self.route_container.clone() {
            location.blocks.response(session, response, self).await;
        }
        crate::rules::response(session, response, self).await;
        for (name, value) in std::mem::take(&mut self.response_headers) {
            let _ = response.append_header(name, value);
        }
    }

    fn add_response_header(&mut self, name: HeaderName, value: HeaderValue) {
        self.response_headers.push((name, value));
    }

    fn block_states(&mut self) -> &mut Vec<(usize, Box<dyn std::any::Any + Send + Sync>)> {
        &mut self.block_states
    }

    fn store(&self) -> Arc<dyn Store> {
        Arc::clone(crate::stores::global::get_store())
    }

    fn response_body_delay(&mut self, session: &Session, body: Option<&Bytes>) -> Option<Duration> {
        let location = self
            .route_container
            .as_ref()
            .filter(|location| location.blocks.paces())
            .cloned();
        let blocks =
            location.and_then(|location| location.blocks.response_body(session, body, self));
        blocks.max(crate::rules::response_body(session, body, self))
    }
}
