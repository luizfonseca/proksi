//! Issuers' published keys (JWKS), one copy per URL whatever the blocks using it, fetched in
//! the background: every 10 minutes, and when a token names a key not seen yet (at most once
//! a minute). A failed fetch keeps the keys fetched before.

use std::{
    collections::HashMap,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, LazyLock, Mutex, PoisonError, Weak,
    },
    time::{Duration, Instant},
};

use arc_swap::ArcSwap;
use jsonwebtoken::{
    jwk::{AlgorithmParameters, JwkSet},
    Algorithm, DecodingKey,
};
use tokio::sync::Notify;

/// How often keys are fetched again
const REFRESH: Duration = Duration::from_secs(600);
/// How soon keys are fetched again for a token naming an unknown key
const REFRESH_UNKNOWN: Duration = Duration::from_secs(60);
/// How long a fetch can take
const FETCH_TIMEOUT: Duration = Duration::from_secs(10);

/// A key of the set, ready to check signatures
pub struct JwksKey {
    pub kid: Option<String>,
    pub key: DecodingKey,
    /// What it can check: its own algorithm if the set says one, else its family's
    pub algorithms: Vec<Algorithm>,
}

pub struct JwksSource {
    url: String,
    keys: ArcSwap<Vec<Arc<JwksKey>>>,
    /// A token named a key the set doesn't have
    unknown: AtomicBool,
    fetched_at: Mutex<Option<Instant>>,
}

/// Every source in use, by URL: dropped with the last block using it
static SOURCES: LazyLock<Mutex<HashMap<String, Weak<JwksSource>>>> = LazyLock::new(Mutex::default);

/// Wakes the refresher: a source was added, or a token named an unknown key
static WAKE: Notify = Notify::const_new();

/// The source of `url`, shared with every other block using it
pub fn source(url: &str) -> Arc<JwksSource> {
    let mut sources = SOURCES.lock().unwrap_or_else(PoisonError::into_inner);
    if let Some(source) = sources.get(url).and_then(Weak::upgrade) {
        return source;
    }
    let source = Arc::new(JwksSource::new(url));
    sources.insert(url.to_string(), Arc::downgrade(&source));
    WAKE.notify_one();
    source
}

impl JwksSource {
    fn new(url: &str) -> Self {
        Self {
            url: url.to_string(),
            keys: ArcSwap::from_pointee(vec![]),
            unknown: AtomicBool::new(false),
            fetched_at: Mutex::new(None),
        }
    }

    /// The key named `kid`, or the only key of a set when the token names none
    ///
    /// # Errors
    ///
    /// When the set has no such key, or several keys and the token names none.
    pub fn key(&self, kid: Option<&str>) -> Result<Arc<JwksKey>, String> {
        let keys = self.keys.load();
        let found = match kid {
            Some(kid) => keys.iter().find(|key| key.kid.as_deref() == Some(kid)),
            None if keys.len() == 1 => keys.first(),
            None => None,
        };
        if let Some(key) = found {
            return Ok(key.clone());
        }
        if !self.unknown.swap(true, Ordering::Relaxed) {
            WAKE.notify_one();
        }
        Err(match kid {
            Some(kid) => format!("no key {kid} in {}", self.url),
            None => format!("the token names no key, and {} has several", self.url),
        })
    }

    /// Replaces the keys with `set`'s (those that can check signatures)
    pub fn set(&self, set: &JwkSet) {
        let keys = set
            .keys
            .iter()
            .filter_map(|jwk| {
                let algorithms = match (&jwk.common.key_algorithm, &jwk.algorithm) {
                    // Secrets aren't published
                    (_, AlgorithmParameters::OctetKey(_) | AlgorithmParameters::Other(_)) => {
                        return None
                    }
                    (Some(algorithm), _) => {
                        vec![Algorithm::try_from(*algorithm).ok()?]
                    }
                    (None, _) => DecodingKey::from_jwk(jwk)
                        .ok()?
                        .family()
                        .algorithms()
                        .to_vec(),
                };
                Some(Arc::new(JwksKey {
                    kid: jwk.common.key_id.clone(),
                    key: DecodingKey::from_jwk(jwk).ok()?,
                    algorithms,
                }))
            })
            .collect();
        self.keys.store(Arc::new(keys));
        self.unknown.store(false, Ordering::Relaxed);
    }

    /// Whether it's time to fetch the keys again
    fn due(&self, now: Instant) -> bool {
        let fetched_at = *self
            .fetched_at
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        match fetched_at {
            None => true,
            Some(at) => {
                let since = now.duration_since(at);
                since >= REFRESH
                    || (self.unknown.load(Ordering::Relaxed) && since >= REFRESH_UNKNOWN)
            }
        }
    }

    async fn fetch(&self, client: &reqwest::Client) {
        *self
            .fetched_at
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = Some(Instant::now());
        let fetched = async {
            client
                .get(&self.url)
                .timeout(FETCH_TIMEOUT)
                .send()
                .await?
                .error_for_status()?
                .json::<JwkSet>()
                .await
        };
        match fetched.await {
            Ok(set) => {
                self.set(&set);
                tracing::debug!(url = %self.url, keys = set.keys.len(), "jwt: keys fetched");
            }
            Err(err) => {
                tracing::warn!(url = %self.url, "jwt: keys can't be fetched, keeping the last ones: {err}");
            }
        }
    }
}

/// Fetches the keys of every source in use, as they're due. Waits, doing nothing, while no
/// block uses a JWKS.
pub async fn refresh() {
    let client = reqwest::Client::new();
    loop {
        let sources = {
            let mut sources = SOURCES.lock().unwrap_or_else(PoisonError::into_inner);
            sources.retain(|_, source| source.strong_count() > 0);
            sources
                .values()
                .filter_map(Weak::upgrade)
                .collect::<Vec<_>>()
        };
        let now = Instant::now();
        for source in sources.iter().filter(|source| source.due(now)) {
            source.fetch(&client).await;
        }
        drop(sources);

        if SOURCES
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .is_empty()
        {
            WAKE.notified().await;
        } else {
            let _ = tokio::time::timeout(Duration::from_secs(5), WAKE.notified()).await;
        }
    }
}

#[cfg(test)]
mod tests {
    use jsonwebtoken::{
        jwk::{Jwk, JwkSet},
        Algorithm, EncodingKey,
    };

    use crate::jwt::verifier::{
        tests::{claims, rsa_pair, token},
        Keys, Verifier,
    };

    use super::source;

    fn jwk(private: &[u8], kid: &str) -> Jwk {
        let mut jwk = Jwk::from_encoding_key(
            &EncodingKey::from_rsa_pem(private).unwrap(),
            Algorithm::RS256,
        )
        .unwrap();
        jwk.common.key_id = Some(kid.to_string());
        jwk
    }

    #[test]
    fn test_one_source_per_url() {
        let one = source("https://keys.test/one");
        let again = source("https://keys.test/one");
        assert!(std::sync::Arc::ptr_eq(&one, &again));
        assert!(!std::sync::Arc::ptr_eq(
            &one,
            &source("https://keys.test/two")
        ));
    }

    #[test]
    fn test_tokens_checked_with_the_key_they_name() {
        let (private, _) = rsa_pair();
        let (other, _) = rsa_pair();
        let jwks = source("https://keys.test/named");
        jwks.set(&JwkSet {
            keys: vec![jwk(&private, "a"), jwk(&other, "b")],
        });
        let verifier = Verifier::new(Keys::Jwks(jwks.clone()), None, None);
        let signed = |private: &[u8], kid: Option<&str>| {
            let mut header = jsonwebtoken::Header::new(Algorithm::RS256);
            header.kid = kid.map(str::to_string);
            jsonwebtoken::encode(
                &header,
                &claims(),
                &EncodingKey::from_rsa_pem(private).unwrap(),
            )
            .unwrap()
        };

        assert!(verifier.verify(&signed(&private, Some("a"))).is_ok());
        assert!(verifier.verify(&signed(&other, Some("b"))).is_ok());
        // Signed by another key than the one it names
        assert!(verifier.verify(&signed(&other, Some("a"))).is_err());
        // An unknown key, and no key named among several
        assert_eq!(
            verifier.verify(&signed(&private, Some("c"))),
            Err("no key c in https://keys.test/named".to_string())
        );
        assert!(verifier.verify(&signed(&private, None)).is_err());
        // Keys published for RS256 only check RS256 tokens
        let key = EncodingKey::from_rsa_pem(&private).unwrap();
        let mut header = jsonwebtoken::Header::new(Algorithm::PS256);
        header.kid = Some("a".to_string());
        let ps256 = jsonwebtoken::encode(&header, &claims(), &key).unwrap();
        assert!(verifier.verify(&ps256).is_err());
        assert!(verifier
            .verify(&token(Algorithm::RS256, &key, &claims()))
            .is_err());
    }

    #[test]
    fn test_the_only_key_checks_tokens_naming_none() {
        let (private, _) = rsa_pair();
        let jwks = source("https://keys.test/only");
        jwks.set(&JwkSet {
            keys: vec![jwk(&private, "a")],
        });
        let verifier = Verifier::new(Keys::Jwks(jwks), None, None);
        let key = EncodingKey::from_rsa_pem(&private).unwrap();
        assert!(verifier
            .verify(&token(Algorithm::RS256, &key, &claims()))
            .is_ok());
    }
}
