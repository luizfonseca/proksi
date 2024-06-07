use std::{
    borrow::Cow,
    fmt::{Display, Formatter},
};

use anyhow::{anyhow, bail, Result};
use async_trait::async_trait;
use cookie::Cookie;
use github::GithubOauthService;
use http::{HeaderName, StatusCode};
use once_cell::sync::Lazy;
use pingora_http::ResponseHeader;
use pingora_proxy::Session;

use workos::WorkosOauthService;

use crate::{config::RoutePlugin, proxy_server::https_proxy::RouterContext};

use super::{jwt, MiddlewarePlugin};

// New providers can be added here
mod github;
mod workos;
//
mod secure_cookie;
mod shared;

/// The HTTP client used to make requests to the Oauth provider.
/// Lazy loaded to avoid creating a new client for each request.
static HTTP_CLIENT: Lazy<reqwest::Client> = Lazy::new(reqwest::Client::new);

const COOKIE_NAME: &str = "__Secure_Auth_PRK_JWT";

#[derive(Debug)]
pub struct UserFromProvider {
    email: Cow<'static, str>,
    team_ids: Vec<String>,
    organization_ids: Vec<String>,
    usernames: Vec<String>,
}

/// The Oauth2 plugin
/// This plugin is responsible for handling Oauth authentication and authorization
/// It can be used to authenticate users against various Oauth providers
/// and authorize them to access specific resources or perform certain actions
/// based on their authorization level.
pub struct Oauth2;

impl Oauth2 {
    pub fn new() -> Self {
        Self {}
    }

    /// Redirects the user to the Oauth provider to authenticate.
    pub async fn redirect_to_oauth_callback(
        &self,
        session: &mut Session,
        oauth_provider: &Provider,
    ) -> Result<bool> {
        let state = "1234";
        // // Step 1.1: If the request does not have the required cookie, block the request
        // // and redirect to the right Oauth Flow
        let mut res_headers =
            ResponseHeader::build_no_case(StatusCode::TEMPORARY_REDIRECT, Some(1))?;

        res_headers.append_header(
            http::header::LOCATION,
            oauth_provider.get_oauth_callback_url(state),
        )?;
        session.write_response_header(Box::new(res_headers)).await?;
        // Finish the request, we don't want to continue processing
        // and the user has been redirected
        return Ok(true);
    }
}

#[derive(Debug, Clone)]
pub struct HeaderExtension {
    name: HeaderName,
    value: String,
}

enum ProviderType {
    Github,
    Workos,
}

impl Display for ProviderType {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            ProviderType::Github => write!(f, "github"),
            ProviderType::Workos => write!(f, "workos"),
        }
    }
}

#[async_trait]
impl MiddlewarePlugin for Oauth2 {
    /// Oauth2 filters requests with/without the required Secure Cookie
    /// If the request has the required cookie, the request is allowed to pass through
    /// and we perform a JWT validation
    /// If the request does not have the required cookie, the request is blocked
    /// and we return a redirect to the oauth login flow (HTTP 307)
    async fn request_filter(
        &self,
        session: &mut Session,
        ctx: &RouterContext,
        plugin: &RoutePlugin,
    ) -> Result<bool> {
        // Nothing to do if the plugin configuration is not present
        if plugin.config.is_none() {
            return Ok(false);
        }

        let plugin_config = plugin
            .config
            .as_ref()
            .ok_or_else(|| anyhow!("Missing config"))?;

        let provider = plugin_config
            .get("provider")
            .ok_or(anyhow!("Missing provider"))?;

        let provider = match provider.as_str() {
            Some("github") => ProviderType::Github,
            Some("workos") => ProviderType::Workos,
            _ => bail!("Provider not found in the plugin configuration"),
        };

        let client_id = shared::from_json_value_to_string(
            plugin_config
                .get("client_id")
                .ok_or_else(|| anyhow!("Missing client_id"))?,
        );
        let client_secret = shared::from_json_value_to_string(
            plugin_config
                .get("client_secret")
                .ok_or_else(|| anyhow!("Missing client_secret"))?,
        );

        let jwt_secret = shared::from_json_value_to_string(
            plugin_config
                .get("jwt_secret")
                .ok_or_else(|| anyhow!("Missing jwt_secret"))?,
        );

        // Callback path based on the selected provider
        let callback_path = format!("/__/oauth/{}/callback", &provider);

        let oauth_provider = Provider {
            client_id,
            client_secret,
            // redirect_uri: "".to_string(),
            typ: provider,
        };

        let uri = &session.req_header().uri;

        // Step 0. Check if the request is for the Oauth Callback URL
        if uri.path() == callback_path {
            tracing::info!("Oauth callback");
            let query = uri
                .query()
                .ok_or_else(|| anyhow!("Missing query in path"))?;

            let query_params = shared::from_string_to_query_params(query);

            let code = query_params
                .get("code")
                .ok_or_else(|| anyhow!("Code not found in the query"))?;

            let state = query_params
                .get("state")
                .ok_or_else(|| anyhow!("State not found in the query"))?;

            // Step 1: Exchange the code for an access token
            let user = oauth_provider.get_oauth_user(code).await.or_else(|err| {
                tracing::error!("Failed to exchange code for token {:?}", err);
                bail!("Failed to exchange code for token");
            })?;

            let jwt_cookie = secure_cookie::create_secure_cookie(user, jwt_secret, &ctx.host)?;

            let mut res_headers = ResponseHeader::build_no_case(StatusCode::FOUND, Some(1))?;
            res_headers.insert_header(http::header::LOCATION, "/")?;
            res_headers.insert_header(http::header::SET_COOKIE, jwt_cookie.to_string())?;
            session.write_response_header(Box::new(res_headers)).await?;

            return Ok(true);
        }

        // Step 1. Check if the request has any cookies
        let cookie_header = session.req_header().headers.get("cookie");
        if cookie_header.is_none() {
            return Ok(self
                .redirect_to_oauth_callback(session, &oauth_provider)
                .await?);
        }
        let cookie_header = cookie_header.unwrap();

        // retrieve required cookie
        let secure_jwt = Cookie::split_parse(cookie_header.to_str()?).find(|c| {
            if let Ok(c) = c {
                c.name() == COOKIE_NAME
            } else {
                false
            }
        });

        // Require cookie is not present
        if secure_jwt.is_none() {
            return Ok(self
                .redirect_to_oauth_callback(session, &oauth_provider)
                .await?);
        }

        let secure_jwt = secure_jwt.unwrap()?;
        let decoded = jwt::decode_jwt(secure_jwt.value(), jwt_secret.as_bytes());

        // Cookie is present but the token is invalid
        if decoded.is_err() {
            let mut res_headers = ResponseHeader::build_no_case(StatusCode::UNAUTHORIZED, Some(1))?;

            let reset_cookie = Cookie::build((COOKIE_NAME, "")).removal().build();

            res_headers.insert_header(http::header::SET_COOKIE, reset_cookie.to_string())?;
            session.write_response_header(Box::new(res_headers)).await?;
            // Finish the request, we don't want to continue processing
            // and the user has been redirected
            return Ok(true);
        }

        // Decoding succeeds and thus the cookie is valid + request can be processed
        Ok(false)
    }

    // Nothing to do after upstream response
    async fn response_filter(
        &self,
        _: &mut Session,
        _: &RouterContext,
        _: &RoutePlugin,
    ) -> Result<bool> {
        Ok(false)
    }
}

pub struct Provider {
    typ: ProviderType,
    client_id: String,
    client_secret: String,
    // redirect_uri: String,
}

impl Provider {
    /// Get the Oauth callback URL for the given provider
    pub fn get_oauth_callback_url(&self, state: &str) -> String {
        match self.typ {
            ProviderType::Github => {
                GithubOauthService::get_oauth_callback_url(&self.client_id, state)
            }
            ProviderType::Workos => {
                WorkosOauthService::get_oauth_callback_url(&self.client_id, state)
            }
        }
    }

    /// Get the Oauth user from the provider using the provided code
    pub async fn get_oauth_user(&self, code: &str) -> Result<UserFromProvider, anyhow::Error> {
        match self.typ {
            ProviderType::Github => {
                GithubOauthService::get_oauth_user(&self.client_id, &self.client_secret, code).await
            }
            ProviderType::Workos => {
                WorkosOauthService::get_oauth_user(&self.client_id, &self.client_secret, code).await
            }
        }
    }
}

// #[cfg(test)]
// mod tests {
//     use super::*;

//     #[test]
//     fn test_new_state() {
//         let module = Oauth2 {};
//     }
// }
