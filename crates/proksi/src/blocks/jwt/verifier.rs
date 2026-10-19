//! Checking a token: its signature, by a key of the family the token says it was signed with
//! (never `none`, and never an HMAC token checked with a public key), then its claims

use std::sync::Arc;

use jsonwebtoken::{decode, decode_header, Algorithm, DecodingKey, Validation};
use serde_json::{Map, Value};

use super::jwks::JwksSource;

/// How long after `exp` (or before `nbf`) a token is still accepted, for clocks a bit apart
const LEEWAY_SECS: u64 = 60;

/// Where the keys checking tokens come from
pub enum Keys {
    /// A secret, or a public key, given in the configuration
    Static(DecodingKey),
    /// The issuer's published keys, fetched in the background
    Jwks(Arc<JwksSource>),
}

impl Keys {
    /// `secret` (HMAC) or `public_key` (PEM: RSA, ECDSA or Ed25519)
    pub fn secret(secret: &str) -> Self {
        Keys::Static(DecodingKey::from_secret(secret.as_bytes()))
    }

    pub fn public_key(pem: &str) -> Result<Self, String> {
        let pem = pem.as_bytes();
        DecodingKey::from_rsa_pem(pem)
            .or_else(|_| DecodingKey::from_ec_pem(pem))
            .or_else(|_| DecodingKey::from_ed_pem(pem))
            .map(Keys::Static)
            .map_err(|_| "public_key is not an RSA, ECDSA or Ed25519 public key in PEM".to_string())
    }
}

pub struct Verifier {
    keys: Keys,
    issuer: Option<String>,
    audience: Option<Vec<String>>,
}

impl Verifier {
    pub fn new(keys: Keys, issuer: Option<String>, audience: Option<Vec<String>>) -> Self {
        Self {
            keys,
            issuer,
            audience,
        }
    }

    /// The claims of `token`, or why it's refused
    pub fn verify(&self, token: &str) -> Result<Map<String, Value>, String> {
        let header = decode_header(token).map_err(|err| format!("malformed token: {err}"))?;
        let found;
        let (key, algorithms): (&DecodingKey, Vec<Algorithm>) = match &self.keys {
            Keys::Static(key) => (key, key.family().algorithms().to_vec()),
            Keys::Jwks(source) => {
                found = source.key(header.kid.as_deref())?;
                (&found.key, found.algorithms.clone())
            }
        };

        let mut validation = Validation::new(header.alg);
        validation.algorithms = algorithms;
        validation.leeway = LEEWAY_SECS;
        validation.validate_nbf = true;
        if let Some(issuer) = &self.issuer {
            validation.set_issuer(&[issuer]);
        }
        match &self.audience {
            Some(audience) => validation.set_audience(audience),
            None => validation.validate_aud = false,
        }

        decode::<Map<String, Value>>(token, key, &validation)
            .map(|data| data.claims)
            .map_err(|err| format!("invalid token: {err}"))
    }
}

#[cfg(test)]
pub mod tests {
    use std::time::{SystemTime, UNIX_EPOCH};

    use jsonwebtoken::{encode, Algorithm, EncodingKey, Header};
    use serde_json::{json, Value};

    use super::{Keys, Verifier};

    pub fn now() -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs()
    }

    pub fn token(alg: Algorithm, key: &EncodingKey, claims: &Value) -> String {
        encode(&Header::new(alg), claims, key).unwrap()
    }

    pub fn claims() -> Value {
        json!({ "sub": "user-1", "iss": "https://auth.test/", "aud": "api", "exp": now() + 600 })
    }

    /// An RSA key pair, as PEM: (private, public)
    pub fn rsa_pair() -> (Vec<u8>, Vec<u8>) {
        let rsa = openssl::rsa::Rsa::generate(2048).unwrap();
        (
            rsa.private_key_to_pem().unwrap(),
            rsa.public_key_to_pem().unwrap(),
        )
    }

    fn hmac() -> Verifier {
        Verifier::new(Keys::secret("secret"), None, None)
    }

    #[test]
    fn test_hmac_tokens() {
        let key = EncodingKey::from_secret(b"secret");
        let verified = hmac()
            .verify(&token(Algorithm::HS256, &key, &claims()))
            .unwrap();
        assert_eq!(verified["sub"], "user-1");
        assert!(hmac()
            .verify(&token(Algorithm::HS512, &key, &claims()))
            .is_ok());

        let other = EncodingKey::from_secret(b"other");
        assert!(hmac()
            .verify(&token(Algorithm::HS256, &other, &claims()))
            .is_err());
        assert!(hmac().verify("not.a.token").is_err());
    }

    #[test]
    fn test_expiry() {
        let key = EncodingKey::from_secret(b"secret");
        let mut expired = claims();
        expired["exp"] = json!(now() - 600);
        assert!(hmac()
            .verify(&token(Algorithm::HS256, &key, &expired))
            .is_err());

        // A clock a bit behind still accepts it
        expired["exp"] = json!(now() - 30);
        assert!(hmac()
            .verify(&token(Algorithm::HS256, &key, &expired))
            .is_ok());

        let mut not_yet = claims();
        not_yet["nbf"] = json!(now() + 600);
        assert!(hmac()
            .verify(&token(Algorithm::HS256, &key, &not_yet))
            .is_err());

        // Tokens must expire
        let mut forever = claims();
        forever.as_object_mut().unwrap().remove("exp");
        assert!(hmac()
            .verify(&token(Algorithm::HS256, &key, &forever))
            .is_err());
    }

    #[test]
    fn test_unsigned_tokens_are_refused() {
        // {"alg":"none","typ":"JWT"}, then the claims, and no signature
        let encode = |value: &Value| {
            openssl::base64::encode_block(value.to_string().as_bytes())
                .trim_end_matches('=')
                .replace('+', "-")
                .replace('/', "_")
        };
        let unsigned = format!(
            "{}.{}.",
            encode(&json!({ "alg": "none", "typ": "JWT" })),
            encode(&claims())
        );
        assert!(hmac().verify(&unsigned).is_err());
    }

    #[test]
    fn test_public_keys() {
        let (private, public) = rsa_pair();
        let verifier = Verifier::new(
            Keys::public_key(std::str::from_utf8(&public).unwrap()).unwrap(),
            None,
            None,
        );
        let key = EncodingKey::from_rsa_pem(&private).unwrap();
        assert!(verifier
            .verify(&token(Algorithm::RS256, &key, &claims()))
            .is_ok());
        assert!(verifier
            .verify(&token(Algorithm::PS384, &key, &claims()))
            .is_ok());

        // An HMAC token "signed" with the public key, as if it were a secret
        let confused = EncodingKey::from_secret(&public);
        assert!(verifier
            .verify(&token(Algorithm::HS256, &confused, &claims()))
            .is_err());

        assert!(Keys::public_key("not a key").is_err());
    }

    #[test]
    fn test_issuer_and_audience() {
        let key = EncodingKey::from_secret(b"secret");
        let verifier = Verifier::new(
            Keys::secret("secret"),
            Some("https://auth.test/".to_string()),
            Some(vec!["api".to_string(), "admin".to_string()]),
        );
        assert!(verifier
            .verify(&token(Algorithm::HS256, &key, &claims()))
            .is_ok());

        let mut other_issuer = claims();
        other_issuer["iss"] = json!("https://evil.test/");
        assert!(verifier
            .verify(&token(Algorithm::HS256, &key, &other_issuer))
            .is_err());

        let mut other_audience = claims();
        other_audience["aud"] = json!(["web"]);
        assert!(verifier
            .verify(&token(Algorithm::HS256, &key, &other_audience))
            .is_err());

        // Without an audience to check, any is accepted
        assert!(hmac()
            .verify(&token(Algorithm::HS256, &key, &other_audience))
            .is_ok());
    }
}
