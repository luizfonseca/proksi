use std::borrow::Cow;

use serde::Deserialize;

use super::{provider::OauthUser, HTTP_CLIENT};

pub(super) struct WorkosOauthService;

const WORKOS_API_URL: &str = "https://api.workos.com/user_management/authorize";

impl WorkosOauthService {
    /// Get the OAuth callback URL for Workos
    pub fn get_oauth_callback_url(client_id: &str, state: &str) -> String {
        format!("{WORKOS_API_URL}?client_id={client_id}&redirect_uri={}&state={state}&provider=authkit&response_type=code","")
    }

    /// Retrieves the user information from Workos
    pub async fn get_oauth_user(
        client_id: &str,
        client_secret: &str,
        code: &str,
    ) -> Result<OauthUser, anyhow::Error> {
        let response = HTTP_CLIENT
            .post(WORKOS_API_URL)
            .json(&serde_json::json!(
                {
                    "client_id": client_id,
                    "client_secret": client_secret,
                    "code": code,
                    "grant_type": "authorization_code",
                    "user-agent": "pingora/0.2.0"
                }
            ))
            .send()
            .await?;
        let body = response.json::<WorkosTokenResponse>().await?;

        Ok(OauthUser {
            email: body.user.email,
            team_ids: vec![],
            organization_ids: vec![],
            usernames: vec![],
        })
    }
}

#[derive(Deserialize)]
struct WorkosTokenResponse {
    // access_token: Cow<'static, str>,
    // refresh_token: Cow<'static, str>,
    user: WorkosUserResponse,
}

#[derive(Deserialize)]
struct WorkosUserResponse {
    // id: Cow<'static, str>,
    email: Cow<'static, str>,
}
