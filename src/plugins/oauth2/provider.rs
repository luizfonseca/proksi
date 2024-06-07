use std::{
    borrow::Cow,
    fmt::{Display, Formatter},
};

use crate::plugins::jwt::JwtClaims;

use super::{github::GithubOauthService, workos::WorkosOauthService};

pub struct Provider {
    pub(super) typ: ProviderType,
    pub(super) client_id: String,
    pub(super) client_secret: String,
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

#[derive(Debug)]
pub struct UserFromProvider {
    pub email: Cow<'static, str>,
    pub team_ids: Vec<String>,
    pub organization_ids: Vec<String>,
    pub usernames: Vec<String>,
}

impl From<JwtClaims> for UserFromProvider {
    fn from(claims: JwtClaims) -> Self {
        Self {
            email: claims.sub,
            team_ids: claims.teams,
            organization_ids: claims.ids,
            usernames: vec![],
        }
    }
}

//
pub enum ProviderType {
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
