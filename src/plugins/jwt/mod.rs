use std::{
    borrow::Cow,
    time::{Duration, SystemTime},
};

use jsonwebtoken::{encode, DecodingKey, EncodingKey, Header, Validation};
use serde::{Deserialize, Serialize};

/// Struct that holds the claims for a JWT token
#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct JwtClaims {
    pub sub: Cow<'static, str>,
    pub exp: usize,
    pub iat: usize,
    pub teams: Vec<String>,
    pub ids: Vec<String>,
    // usernames: Vec<String>,
}

/// Generates a JWT token for the given sub
pub(crate) fn encode_jwt(sub: &str, secret: &[u8]) -> Result<String, anyhow::Error> {
    let start = SystemTime::now();
    let since = start.duration_since(SystemTime::UNIX_EPOCH)?;

    let one_day_in_secs = 60 * 60 * 24;
    let in_one_day = since
        .checked_add(Duration::from_secs(one_day_in_secs))
        .unwrap();

    let claims = JwtClaims {
        sub: Cow::Owned(sub.to_string()),
        exp: in_one_day.as_secs() as usize,
        iat: since.as_secs() as usize,
        teams: vec![],
        ids: vec![],
        // usernames: vec![],
    };

    Ok(encode(
        &Header::default(),
        &claims,
        &EncodingKey::from_secret(secret),
    )?)
}

/// Decodes a given JWT token
pub(crate) fn decode_jwt(token: &str, secret: &[u8]) -> Result<JwtClaims, anyhow::Error> {
    let data = jsonwebtoken::decode::<JwtClaims>(
        token,
        &DecodingKey::from_secret(secret),
        &Validation::default(),
    )?;

    Ok(data.claims)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_encode_jwt_with_secret() {
        let token = encode_jwt("test", b"secret");
        assert_eq!(token.is_ok(), true);
    }
}
