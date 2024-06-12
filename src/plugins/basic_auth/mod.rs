use std::{borrow::Cow, collections::HashMap};

use async_trait::async_trait;
use http::{header, StatusCode};
use openssl::base64;
use pingora::{
    http::{RequestHeader, ResponseHeader},
    proxy::Session,
};

use crate::{config::RoutePlugin, proxy_server::https_proxy::RouterContext};

use super::MiddlewarePlugin;

pub struct BasicAuth;
impl BasicAuth {
    pub fn new() -> Self {
        Self {}
    }

    /// Returns a WWW-Authenticate header response indicating to downstream that
    /// This request requires basic auth
    fn respond_with_authenticate(host: &str) -> anyhow::Result<Box<ResponseHeader>> {
        let mut res_headers = ResponseHeader::build_no_case(StatusCode::UNAUTHORIZED, Some(1))?;
        let realm = format!("Basic realm=\"{host}\", charset=\"UTF-8\"");
        res_headers.insert_header(header::WWW_AUTHENTICATE, &realm)?;

        Ok(Box::new(res_headers))
    }

    /// Extracts the 'user' and 'pass' from the plugin configuration
    fn get_auth_config(
        config: &HashMap<Cow<'static, str>, serde_json::Value>,
    ) -> Option<(String, String)> {
        let user = config.get("user")?.as_str()?.to_string();
        let pass = config.get("pass")?.as_str()?.to_string();
        Some((user, pass))
    }

    /// Validates the 'Authorization' header against the configured 'user' and 'pass'
    fn validate_auth_header(auth_header: &str, user: &str, pass: &str) -> anyhow::Result<bool> {
        let encoded = auth_header.trim_start_matches("Basic ");
        let decoded = String::from_utf8(base64::decode_block(encoded)?)?;
        let (auth_user, auth_pass) = decoded.split_once(':').unwrap_or_default();
        Ok(auth_user == user && auth_pass == pass)
    }
}

#[async_trait]
impl MiddlewarePlugin for BasicAuth {
    async fn request_filter(
        &self,
        session: &mut Session,
        ctx: &mut RouterContext,
        plugin: &RoutePlugin,
    ) -> anyhow::Result<bool> {
        if plugin.config.is_none() {
            // Nothing to do if the plugin configuration is not present
            return Ok(false);
        }

        let config = plugin.config.as_ref().unwrap();

        let Some((user, pass)) = Self::get_auth_config(config) else {
            session
                .write_response_header(Self::respond_with_authenticate(&ctx.host)?)
                .await?;
            return Ok(true);
        };

        // Get auth header but if missing returns 401
        let auth_header = if let Some(header) = session.req_header().headers.get("authorization") {
            header.to_str()?
        } else {
            session
                .write_response_header(Self::respond_with_authenticate(&ctx.host)?)
                .await?;
            return Ok(true);
        };

        if !auth_header.starts_with("Basic ")
            || !Self::validate_auth_header(auth_header, &user, &pass)?
        {
            session
                .write_response_header(Self::respond_with_authenticate(&ctx.host)?)
                .await?;
            return Ok(true);
        }

        Ok(false)
    }

    async fn upstream_request_filter(
        &self,
        _: &mut Session,
        _: &mut RequestHeader,
        _: &mut RouterContext,
    ) -> anyhow::Result<()> {
        Ok(())
    }

    // Nothing to do after upstream response
    async fn response_filter(
        &self,
        _: &mut Session,
        _: &mut RouterContext,
        _: &RoutePlugin,
    ) -> anyhow::Result<bool> {
        Ok(false)
    }

    fn upstream_response_filter(
        &self,
        _: &mut Session,
        _: &mut ResponseHeader,
        _: &mut RouterContext,
    ) -> anyhow::Result<()> {
        //
        Ok(())
    }
}
