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
        .ok_or_else(|| anyhow::anyhow!("JWT expiration out of range"))?;

    let claims = JwtClaims {
        sub: Cow::Owned(sub.to_string()),
        exp: usize::try_from(in_one_day.as_secs())?,
        iat: usize::try_from(since.as_secs())?,
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
        assert!(token.is_ok());
    }

    #[test]
    fn test_encode_and_decode_jwt() {
        let token = encode_jwt("user@example.com", b"secret").unwrap();
        let claims = decode_jwt(&token, b"secret").unwrap();

        assert_eq!(claims.sub, "user@example.com");
        assert!(decode_jwt(&token, b"wrong-secret").is_err());
    }

    #[test]
    fn test_decode_jwt_issued_by_jsonwebtoken_v9() {
        // Issued by jsonwebtoken 9.3.1, so existing sessions stay valid after upgrading
        let token = "eyJ0eXAiOiJKV1QiLCJhbGciOiJIUzI1NiJ9.eyJzdWIiOiJ1c2VyQGV4YW1wbGUuY29tIiwiZXhwIjo0MTAyNDQ0ODAwLCJpYXQiOjE3NjAwMDAwMDAsInRlYW1zIjpbXSwiaWRzIjpbXX0.a2N3dj2XVgAEq41Aqm228d8VfH4uBhYCSqQDP6z2loY";
        let claims = decode_jwt(token, b"proksi-test-secret").unwrap();

        assert_eq!(claims.sub, "user@example.com");
        assert_eq!(claims.exp, 4_102_444_800);
    }
}
