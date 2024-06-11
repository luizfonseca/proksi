use std::{borrow::Cow, collections::HashMap, sync::Arc};

use anyhow::{anyhow, bail, Result};
use async_trait::async_trait;
use cookie::Cookie;
use dashmap::DashMap;
use http::StatusCode;
use once_cell::sync::Lazy;
use pingora_http::{RequestHeader, ResponseHeader};
use pingora_proxy::Session;

use provider::{OauthType, OauthUser, Provider};

use crate::{config::RoutePlugin, proxy_server::https_proxy::RouterContext};

use super::{jwt, MiddlewarePlugin};

// New providers can be added here
mod github;
mod workos;
//
mod provider;
mod secure_cookie;
mod shared;

/// The HTTP client used to make requests to the Oauth provider.
/// Lazy loaded to avoid creating a new client for each request.
static HTTP_CLIENT: Lazy<reqwest::Client> = Lazy::new(reqwest::Client::new);

// Shared state for Oauth2 flows (should be cleaned up after fetching for the first time)
// TODO find a way to clean up/expire the state
static OAUTH2_STATE: Lazy<Arc<DashMap<String, String>>> = Lazy::new(|| Arc::new(DashMap::new()));
const COOKIE_NAME: &str = "__Secure_Auth_PRK_JWT";

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

    /// Checks if the user is authorized to access the protected Oauth2 resource
    /// This is part of the validation object in the oauth2 configuration.
    fn is_authorized(user: &OauthUser, validations: Option<&serde_json::Value>) -> bool {
        shared::validate_user_from_provider(user, validations)
    }

    /// Redirects the user to the Oauth provider to authenticate.
    async fn redirect_to_oauth_callback(
        &self,
        session: &mut Session,
        oauth_provider: &Provider,
    ) -> Result<bool> {
        let current_address = session.req_header().uri.to_string();
        // let host = session.req_header().uri.host().unwrap_or_default();
        let state = uuid::Uuid::new_v4().to_string();

        let mut res_headers =
            ResponseHeader::build_no_case(StatusCode::TEMPORARY_REDIRECT, Some(1))?;

        // Removes any cookie to prevent the user from being redirected
        // to the Oauth provider over and over
        // let removed_cookie = remove_secure_cookie(host);
        // res_headers.append_header(http::header::SET_COOKIE, removed_cookie.to_string())?;

        res_headers.append_header(
            http::header::LOCATION,
            oauth_provider.get_oauth_callback_url(&state),
        )?;
        res_headers.append_header(
            http::header::CACHE_CONTROL,
            "no-store, no-cache, must-revalidate, max-age=0",
        )?;

        // Store the current path in the state
        OAUTH2_STATE.insert(state, current_address);
        session.write_response_header(Box::new(res_headers)).await?;

        // Finish the request, we don't want to continue processing
        // and the user has been redirected
        Ok(true)
    }

    /// Ends the Oauth2 flow and returns HTTP unauthorized if errors occur during the Oauth process
    async fn unauthorized_response(&self, session: &mut Session) -> Result<bool> {
        let res_headers = ResponseHeader::build_no_case(StatusCode::UNAUTHORIZED, Some(1))?;

        // Store the current path in the stat
        session.write_response_header(Box::new(res_headers)).await?;

        // Finish the request, we don't want to continue processing
        // and the user has been redirected
        Ok(true)
    }

    /// Validates a given cookie and returns true if
    /// the user is authorized
    async fn validate_cookie(
        &self,
        session: &mut Session,
        jwt_secret: &str,
        validations: Option<&serde_json::Value>,
    ) -> Result<bool> {
        let cookie_header = session.req_header().headers.get("cookie");
        if cookie_header.is_none() {
            return Ok(false); // will redirect to oauth callback
        }

        let secure_jwt = Cookie::split_parse(cookie_header.unwrap().to_str()?)
            .filter_map(Result::ok)
            .find(|c| c.name() == COOKIE_NAME);

        if secure_jwt.is_none() {
            return Ok(false); // will redirect to oauth callback
        }

        let decoded = jwt::decode_jwt(secure_jwt.unwrap().value(), jwt_secret.as_bytes());

        // Token expired or another err
        if decoded.is_err() {
            return Ok(false); // will redirect to oauth callback
        }

        if !Self::is_authorized(&decoded?.into(), validations) {
            return self.unauthorized_response(session).await;
        }

        Ok(true)
    }

    fn get_required_config(
        plugin_config: &HashMap<Cow<'static, str>, serde_json::Value>,
        key: &str,
    ) -> Result<String> {
        plugin_config
            .get(key)
            .and_then(|v| v.as_str())
            .map(ToString::to_string)
            .ok_or_else(|| anyhow!("Missing or invalid {}", key))
    }

    fn parse_provider(
        plugin_config: &HashMap<Cow<'static, str>, serde_json::Value>,
    ) -> Result<OauthType> {
        let provider = plugin_config
            .get("provider")
            .and_then(|v| v.as_str())
            .ok_or(anyhow!("Missing or invalid provider"))?;

        match provider {
            "github" => Ok(OauthType::Github),
            "workos" => Ok(OauthType::Workos),
            _ => bail!("Provider not found in the plugin configuration"),
        }
    }
}

#[async_trait]
impl MiddlewarePlugin for Oauth2 {
    async fn upstream_request_filter(
        &self,
        _: &mut Session,
        _: &mut RequestHeader,
        _: &mut RouterContext,
    ) -> Result<()> {
        Ok(())
    }

    fn upstream_response_filter(
        &self,
        _: &mut Session,
        _: &mut ResponseHeader,
        _: &mut RouterContext,
    ) -> Result<()> {
        Ok(())
    }

    /// Oauth2 filters requests with/without the required Secure Cookie
    /// If the request has the required cookie, the request is allowed to pass through
    /// and we perform a JWT validation
    /// If the request does not have the required cookie, the request is blocked
    /// and we return a redirect to the oauth login flow (HTTP 307)
    async fn request_filter(
        &self,
        session: &mut Session,
        ctx: &mut RouterContext,
        plugin: &RoutePlugin,
    ) -> Result<bool> {
        // Nothing to do if the plugin configuration is not present
        if plugin.config.is_none() {
            return Ok(false);
        }

        let plugin_config = plugin.config.as_ref().unwrap();

        let provider = Self::parse_provider(plugin_config)?;

        let client_id = Self::get_required_config(plugin_config, "client_id")?;
        let client_secret = Self::get_required_config(plugin_config, "client_secret")?;
        let jwt_secret = Self::get_required_config(plugin_config, "jwt_secret")?;
        let validations = plugin_config.get("validations");

        // Callback path based on the selected provider
        let callback_path = format!("/__/oauth/{}/callback", &provider);

        // Create provider service
        let oauth_provider = Provider {
            client_id,
            client_secret,
            typ: provider,
        };

        let uri = &session.req_header().uri;

        // Step 0. Check if the request is for the Oauth Callback URL
        if uri.path() == callback_path {
            let Some(query) = uri.query() else {
                return self.unauthorized_response(session).await;
            };

            let query_params = shared::from_string_to_query_params(query);

            let Some(code) = query_params.get("code") else {
                tracing::info!("missing code in the query");
                return self.unauthorized_response(session).await;
            };

            let Some(state) = query_params.get("state") else {
                tracing::info!("missing state in the query");
                return self.unauthorized_response(session).await;
            };

            let Some(saved_state) = OAUTH2_STATE.get(&state.to_string()) else {
                tracing::info!("state does not exist or was removed");
                return self.unauthorized_response(session).await;
            };

            let redirect_from_state = saved_state.value().to_owned();

            // Remove ref of state to avoid deadlocks
            drop(saved_state);

            // Cleanup state to avoid replay attacks
            OAUTH2_STATE.remove(&state.to_string());

            // Step 1: Exchange the code for an access token
            let user = match oauth_provider.get_oauth_user(code).await {
                Err(err) => {
                    tracing::error!("Failed to exchange code {code}, state {state}: {err}");
                    return self.unauthorized_response(session).await;
                }
                Ok(user) => user,
            };

            // Validate if user is authorized to access the protected resource
            if !Self::is_authorized(&user, validations) {
                tracing::info!("user is not authorized {:?}", user);
                return self.unauthorized_response(session).await;
            }

            let jwt_cookie = secure_cookie::create_secure_cookie(&user, &jwt_secret, &ctx.host)?;

            let mut res_headers = ResponseHeader::build_no_case(StatusCode::FOUND, Some(1))?;
            res_headers.insert_header(http::header::SET_COOKIE, jwt_cookie.to_string())?;
            res_headers.insert_header(http::header::LOCATION, redirect_from_state)?;
            res_headers.insert_header(
                http::header::CACHE_CONTROL,
                "no-store, no-cache, must-revalidate, max-age=0",
            )?;

            session.write_response_header(Box::new(res_headers)).await?;

            return Ok(true);
        }

        if self
            .validate_cookie(session, &jwt_secret, validations)
            .await?
        {
            // If the user is not authorized, return true to
            // interrupt the request
            return Ok(false);
        }

        self.redirect_to_oauth_callback(session, &oauth_provider)
            .await
    }

    // Nothing to do after upstream response
    async fn response_filter(
        &self,
        _: &mut Session,
        _: &mut RouterContext,
        _: &RoutePlugin,
    ) -> Result<bool> {
        Ok(false)
    }
}
