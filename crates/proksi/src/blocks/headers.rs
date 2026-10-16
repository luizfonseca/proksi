//! `headers { response { add = { ... }, remove = [...] } }`: changes response headers

use std::str::FromStr;

use anyhow::Result;
use async_trait::async_trait;
use http::{HeaderName, HeaderValue};
use pingora::{http::ResponseHeader, proxy::Session};

use crate::{config::RouteHeader, proxy_server::https_proxy::RouterContext};

use super::Middleware;

pub(super) struct Headers {
    add: Vec<(HeaderName, HeaderValue)>,
    remove: Vec<String>,
}

impl Headers {
    /// `None` when no header is changed
    pub(super) fn new(headers: &RouteHeader) -> Result<Option<Self>, String> {
        let add = headers
            .add
            .iter()
            .flatten()
            .map(|header| {
                let name = HeaderName::from_str(&header.name);
                let value = HeaderValue::from_str(&header.value);
                match (name, value) {
                    (Ok(name), Ok(value)) => Ok((name, value)),
                    _ => Err(format!("headers: {:?} is not a valid header", header.name)),
                }
            })
            .collect::<Result<Vec<_>, _>>()?;
        let remove = headers
            .remove
            .iter()
            .flatten()
            .map(|header| header.name.to_string())
            .collect::<Vec<_>>();

        if add.is_empty() && remove.is_empty() {
            return Ok(None);
        }
        Ok(Some(Self { add, remove }))
    }
}

#[async_trait]
impl Middleware for Headers {
    async fn response(
        &self,
        _session: &mut Session,
        response: &mut ResponseHeader,
        _ctx: &mut RouterContext,
    ) -> Result<()> {
        for (name, value) in &self.add {
            response.insert_header(name, value)?;
        }
        for name in &self.remove {
            response.remove_header(name);
        }
        Ok(())
    }
}
