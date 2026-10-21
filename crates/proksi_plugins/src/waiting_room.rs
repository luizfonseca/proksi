//! `waiting_room { active = 500, on_store_error = "admit" }`: lets `active` visitors in at once;
//! the others wait, in the order they came, on a page that refreshes until it's their turn.
//!
//! The room (who holds a slot, who waits) is kept in the store, so replicas sharing a Redis or
//! SQL store share it. Visitors carry a pass in a cookie, encrypted and authenticated with
//! AES-256-GCM: an admitted visitor's pass is checked without asking the store, until it's
//! close to expiring.

use std::{
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use anyhow::Result;
use async_trait::async_trait;
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use http::{header, HeaderMap, HeaderValue, StatusCode};
use pingora::{http::ResponseHeader, proxy::Session};
use proksi_store::{Admission, RoomLimits, Store};
use ring::{
    aead::{Aad, LessSafeKey, Nonce, UnboundKey, AES_256_GCM},
    rand::{SecureRandom, SystemRandom},
};
use tokio::sync::OnceCell;

use crate::{
    rate_limit::parse_duration, respond::respond_with_body, settings::OnStoreError, RequestCtx,
    RoutePlugin,
};

use super::Middleware;

const NONCE: usize = 12;
const TAG: usize = 16;
/// Kind (1), visitor (16), expiry (8, seconds since the epoch), room (8)
const PASS: usize = 33;
const SEALED: usize = NONCE + PASS + TAG;

const ADMITTED: u8 = 1;
const WAITING: u8 = 0;

/// How long a call to the store can take before `on_store_error` applies
const STORE_TIMEOUT: Duration = Duration::from_secs(2);

const PAGE: &str = include_str!("waiting_room.html");

pub(super) struct WaitingRoom {
    /// The room, within a host
    name: String,
    /// `name` hashed, carried in passes so they're good for this room only
    tag: [u8; 8],
    cookie: String,
    limits: RoomLimits,
    /// Admitted visitors ask the store again this close to their pass expiring
    renew: u64,
    refresh: Duration,
    page: String,
    on_store_error: OnStoreError,
    /// The key from `secret`, or else the store's, once read
    secret: Option<[u8; 32]>,
    key: OnceCell<LessSafeKey>,
    random: SystemRandom,
}

/// What a visitor's cookie says
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Pass {
    kind: u8,
    visitor: [u8; 16],
    /// Seconds since the epoch: when an admitted visitor's slot expires
    expires: u64,
}

impl WaitingRoom {
    pub(super) fn new(plugin: &RoutePlugin, location_path: &str) -> Result<Self, String> {
        let settings: super::settings::WaitingRoom = super::settings::read("waiting_room", plugin)?;
        let active = Some(settings.active)
            .filter(|active| *active > 0)
            .ok_or("waiting_room: `active` must be a number greater than 0")?;
        let duration = |name: &str, value: Option<&str>, default: u64| match value {
            None => Ok(Duration::from_secs(default)),
            Some(value) => parse_duration(value).ok_or_else(|| {
                format!(
                    "waiting_room: `{name}` must be a duration such as \"10s\", \"5m\" or \"1h\""
                )
            }),
        };
        let session = duration("session", settings.session.as_deref(), 300)?;
        let refresh = duration("refresh", settings.refresh.as_deref(), 10)?;
        let page = match &settings.page {
            None => PAGE.to_string(),
            Some(file) => std::fs::read_to_string(file)
                .map_err(|err| format!("waiting_room: can't read the page {file:?}: {err}"))?,
        };
        let name = settings.name.unwrap_or_else(|| location_path.to_string());
        let tag = fnv1a(name.as_bytes());

        Ok(Self {
            cookie: format!("proksi_room_{}", hex8(tag)),
            name,
            tag: tag.to_le_bytes(),
            limits: RoomLimits {
                active,
                session,
                abandon: refresh.saturating_mul(3),
            },
            renew: (session / 5).as_secs().max(1),
            refresh,
            page,
            on_store_error: settings.on_store_error,
            secret: settings.secret.map(|secret| {
                let digest = ring::digest::digest(&ring::digest::SHA256, secret.as_bytes());
                let mut key = [0; 32];
                key.copy_from_slice(digest.as_ref());
                key
            }),
            key: OnceCell::new(),
            random: SystemRandom::new(),
        })
    }

    /// The key of the passes, read from the store the first time
    async fn key(&self, store: Arc<dyn Store>) -> Result<&LessSafeKey, String> {
        self.key
            .get_or_try_init(|| async {
                let key = match self.secret {
                    Some(key) => key,
                    None => off_thread(async move { store.waiting_room_key().await }).await?,
                };
                let key = UnboundKey::new(&AES_256_GCM, &key).map_err(|_| "invalid key")?;
                Ok(LessSafeKey::new(key))
            })
            .await
    }

    /// The visitor's pass, from their cookie, if it's this room's and wasn't changed. `host`
    /// is authenticated with it: a pass is only good for the host it was given for.
    fn pass(&self, key: &LessSafeKey, headers: &HeaderMap, host: &str) -> Option<Pass> {
        let encoded = cookie(headers, &self.cookie)?;
        let mut sealed = [0; SEALED + 3];
        let len = URL_SAFE_NO_PAD.decode_slice(encoded, &mut sealed).ok()?;
        let (nonce, rest) = sealed.get_mut(..len)?.split_first_chunk_mut::<NONCE>()?;
        let nonce = Nonce::assume_unique_for_key(*nonce);
        let plain = key
            .open_in_place(nonce, Aad::from(host.as_bytes()), rest)
            .ok()?;
        let (&mut [kind], rest) = plain.split_first_chunk_mut::<1>()?;
        let (visitor, rest) = rest.split_first_chunk::<16>()?;
        let (expires, room) = rest.split_first_chunk::<8>()?;
        (room == self.tag).then_some(Pass {
            kind,
            visitor: *visitor,
            expires: u64::from_le_bytes(*expires),
        })
    }

    /// `pass` as a `Set-Cookie` value, kept by the browser for `max_age`
    fn seal(
        &self,
        key: &LessSafeKey,
        pass: Pass,
        host: &str,
        max_age: Duration,
        secure: bool,
    ) -> Option<HeaderValue> {
        let mut sealed = [0; SEALED];
        let (nonce, rest) = sealed.split_first_chunk_mut::<NONCE>()?;
        self.random.fill(nonce).ok()?;
        let nonce = Nonce::assume_unique_for_key(*nonce);
        let (plain, tag) = rest.split_at_mut_checked(PASS)?;
        let (kind, fields) = plain.split_first_mut()?;
        *kind = pass.kind;
        let (visitor, fields) = fields.split_first_chunk_mut::<16>()?;
        *visitor = pass.visitor;
        let (expires, room) = fields.split_first_chunk_mut::<8>()?;
        *expires = pass.expires.to_le_bytes();
        room.copy_from_slice(&self.tag);
        let sealed_tag = key
            .seal_in_place_separate_tag(nonce, Aad::from(host.as_bytes()), plain)
            .ok()?;
        tag.copy_from_slice(sealed_tag.as_ref());
        let secure = if secure { "; Secure" } else { "" };
        let cookie = format!(
            "{}={}; Path=/; Max-Age={}; HttpOnly; SameSite=Lax{secure}",
            self.cookie,
            URL_SAFE_NO_PAD.encode(sealed),
            max_age.as_secs()
        );
        HeaderValue::from_str(&cookie).ok()
    }

    fn new_visitor(&self) -> Option<[u8; 16]> {
        let mut visitor = [0; 16];
        self.random.fill(&mut visitor).ok()?;
        Some(visitor)
    }

    /// The waiting page, answered with `503` (with the visitor's place, when known)
    async fn wait(
        &self,
        session: &mut Session,
        ctx: &mut dyn RequestCtx,
        cookie: Option<HeaderValue>,
        place: Option<(u64, u64)>,
    ) -> Result<bool> {
        let (position, waiting) = place.map_or_else(
            || ("…".to_string(), "…".to_string()),
            |(position, waiting)| (position.to_string(), waiting.to_string()),
        );
        let refresh = self.refresh.as_secs().to_string();
        let body = self
            .page
            .replace("{position}", &position)
            .replace("{waiting}", &waiting)
            .replace("{refresh}", &refresh);

        let mut response = ResponseHeader::build_no_case(StatusCode::SERVICE_UNAVAILABLE, Some(5))?;
        response.insert_header(header::CONTENT_TYPE, "text/html; charset=utf-8")?;
        response.insert_header(header::CACHE_CONTROL, "no-store")?;
        response.insert_header(header::RETRY_AFTER, refresh)?;
        if let Some(cookie) = cookie {
            response.insert_header(header::SET_COOKIE, cookie)?;
        }
        respond_with_body(session, ctx, response, body.into()).await?;
        Ok(true)
    }

    /// When the store can't be reached (or the key read): `on_store_error`
    async fn store_failed(
        &self,
        session: &mut Session,
        ctx: &mut dyn RequestCtx,
        err: &str,
    ) -> Result<bool> {
        tracing::warn!(
            room = self.name,
            "waiting_room: the store can't be reached: {err}"
        );
        match self.on_store_error {
            OnStoreError::Admit => Ok(false),
            OnStoreError::Wait => self.wait(session, ctx, None, None).await,
        }
    }
}

#[async_trait]
impl Middleware for WaitingRoom {
    async fn request(&self, session: &mut Session, ctx: &mut dyn RequestCtx) -> Result<bool> {
        let key = match self.key(ctx.store()).await {
            Ok(key) => key,
            Err(err) => return self.store_failed(session, ctx, &err).await,
        };
        let now = unix_seconds();
        let pass = self.pass(key, &session.req_header().headers, ctx.host());
        // Admitted, and not about to expire: no need to ask the store
        if pass.is_some_and(|pass| pass.kind == ADMITTED && pass.expires > now + self.renew) {
            return Ok(false);
        }

        let Some(visitor) = pass.map(|pass| pass.visitor).or_else(|| self.new_visitor()) else {
            return self
                .store_failed(session, ctx, "no random visitor id")
                .await;
        };
        let (store, room, limits) = (
            ctx.store(),
            format!("{}{}", ctx.host(), self.name),
            self.limits,
        );
        let id = hex(&visitor);
        let admission =
            off_thread(async move { store.wait_in_room(&room, &id, limits).await }).await;
        let secure = session
            .digest()
            .is_some_and(|digest| digest.ssl_digest.is_some());
        match admission {
            Ok(Admission::Admitted) => {
                let pass = Pass {
                    kind: ADMITTED,
                    visitor,
                    expires: now + self.limits.session.as_secs(),
                };
                let cookie = self.seal(key, pass, ctx.host(), self.limits.session, secure);
                if let Some(cookie) = cookie {
                    ctx.add_response_header(header::SET_COOKIE, cookie);
                }
                Ok(false)
            }
            Ok(Admission::Waiting { position, waiting }) => {
                let pass = Pass {
                    kind: WAITING,
                    visitor,
                    expires: 0,
                };
                let cookie = self.seal(key, pass, ctx.host(), self.limits.abandon, secure);
                self.wait(session, ctx, cookie, Some((position, waiting)))
                    .await
            }
            Err(err) => self.store_failed(session, ctx, &err).await,
        }
    }
}

/// Runs a store call on Tokio's blocking threads (stores may block), with [`STORE_TIMEOUT`]
async fn off_thread<T: Send + 'static>(
    call: impl std::future::Future<Output = Result<T, proksi_store::StoreError>> + Send + 'static,
) -> Result<T, String> {
    let task = tokio::task::spawn_blocking(|| tokio::runtime::Handle::current().block_on(call));
    match tokio::time::timeout(STORE_TIMEOUT, task).await {
        Ok(Ok(result)) => result.map_err(|err| err.to_string()),
        Ok(Err(err)) => Err(err.to_string()),
        Err(_) => Err("timed out".to_string()),
    }
}

/// The value of the cookie `name`, among the request's
fn cookie<'a>(headers: &'a HeaderMap, name: &str) -> Option<&'a str> {
    headers
        .get_all(header::COOKIE)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .flat_map(|cookies| cookies.split(';'))
        .find_map(|pair| pair.trim().strip_prefix(name)?.strip_prefix('='))
}

fn unix_seconds() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_secs())
}

/// FNV-1a: the same on every server, unlike Rust's hashers
fn fnv1a(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325, |hash, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(0x0100_0000_01b3)
    })
}

fn hex8(value: u64) -> String {
    format!("{:08x}", value >> 32)
}

fn hex(bytes: &[u8; 16]) -> String {
    use std::fmt::Write;
    bytes.iter().fold(String::with_capacity(32), |mut text, byte| {
        let _ = write!(text, "{byte:02x}");
        text
    })
}

#[cfg(test)]
mod tests {
    use http::HeaderMap;
    use ring::aead::{LessSafeKey, UnboundKey, AES_256_GCM};

    use crate::RoutePlugin;

    use super::{Pass, WaitingRoom, ADMITTED, WAITING};

    fn room(config: serde_json::Value) -> Result<WaitingRoom, String> {
        WaitingRoom::new(
            &RoutePlugin {
                name: "waiting_room".into(),
                config: serde_json::from_value(config).unwrap(),
            },
            "/shop",
        )
    }

    fn key() -> LessSafeKey {
        LessSafeKey::new(UnboundKey::new(&AES_256_GCM, &[7; 32]).unwrap())
    }

    /// The request headers of a browser sending the cookie of `set_cookie`
    fn sent(set_cookie: &http::HeaderValue) -> HeaderMap {
        let pair = set_cookie.to_str().unwrap().split(';').next().unwrap();
        let mut headers = HeaderMap::new();
        headers.insert("cookie", format!("theme=dark; {pair}").parse().unwrap());
        headers
    }

    const PASS: Pass = Pass {
        kind: ADMITTED,
        visitor: [9; 16],
        expires: 1_900_000_000,
    };

    #[test]
    fn test_settings() {
        let block = room(serde_json::json!({ "active": 500, "on_store_error": "admit" })).unwrap();
        assert_eq!(block.limits.active, 500);
        assert_eq!(block.limits.session.as_secs(), 300);
        assert_eq!(block.limits.abandon.as_secs(), 30);
        assert_eq!(block.renew, 60);
        assert_eq!(block.name, "/shop");
        assert!(block.cookie.starts_with("proksi_room_"));

        for (config, err) in [
            (
                serde_json::json!({ "active": 0, "on_store_error": "admit" }),
                "waiting_room: `active` must be a number greater than 0",
            ),
            (
                serde_json::json!({ "active": 1, "on_store_error": "admit", "session": "soon" }),
                "waiting_room: `session` must be a duration such as \"10s\", \"5m\" or \"1h\"",
            ),
            (
                serde_json::json!({ "active": 1 }),
                "waiting_room: `on_store_error` is required",
            ),
        ] {
            assert_eq!(room(config).err().as_deref(), Some(err));
        }
        let unknown = room(serde_json::json!({ "active": 1, "on_store_error": "maybe" })).err();
        assert!(unknown.is_some_and(|err| err.contains("unknown variant `maybe`")));
    }

    #[test]
    fn test_a_pass_reads_back() {
        let block = room(serde_json::json!({ "active": 1, "on_store_error": "wait" })).unwrap();
        let cookie = block
            .seal(&key(), PASS, "shop.test", block.limits.session, true)
            .unwrap();
        let text = cookie.to_str().unwrap();
        assert!(
            text.ends_with("; Path=/; Max-Age=300; HttpOnly; SameSite=Lax; Secure"),
            "{text}"
        );
        assert_eq!(block.pass(&key(), &sent(&cookie), "shop.test"), Some(PASS));
        // A new nonce each time
        let again = block
            .seal(&key(), PASS, "shop.test", block.limits.session, true)
            .unwrap();
        assert_ne!(cookie, again);
    }

    #[test]
    fn test_passes_cant_be_changed_or_moved() {
        let block = room(serde_json::json!({ "active": 1, "on_store_error": "wait" })).unwrap();
        let waiting = Pass {
            kind: WAITING,
            ..PASS
        };
        let cookie = block
            .seal(&key(), waiting, "shop.test", block.limits.abandon, false)
            .unwrap();
        let headers = sent(&cookie);

        // Another host, another key, another room
        assert_eq!(block.pass(&key(), &headers, "other.test"), None);
        let other_key = LessSafeKey::new(UnboundKey::new(&AES_256_GCM, &[8; 32]).unwrap());
        assert_eq!(block.pass(&other_key, &headers, "shop.test"), None);
        let other_room =
            room(serde_json::json!({ "active": 1, "on_store_error": "wait", "name": "sale" }))
                .unwrap();
        let renamed = headers
            .get("cookie")
            .unwrap()
            .to_str()
            .unwrap()
            .replace(&block.cookie, &other_room.cookie);
        let mut moved = HeaderMap::new();
        moved.insert("cookie", renamed.parse().unwrap());
        assert_eq!(other_room.pass(&key(), &moved, "shop.test"), None);

        // Any byte changed (here, the kind: waiting to admitted)
        let value = cookie.to_str().unwrap().split(';').next().unwrap();
        let (name, encoded) = value.split_once('=').unwrap();
        let mut bytes =
            base64::Engine::decode(&base64::engine::general_purpose::URL_SAFE_NO_PAD, encoded)
                .unwrap();
        bytes[super::NONCE] ^= 1;
        let tampered =
            base64::Engine::encode(&base64::engine::general_purpose::URL_SAFE_NO_PAD, bytes);
        let mut changed = HeaderMap::new();
        changed.insert("cookie", format!("{name}={tampered}").parse().unwrap());
        assert_eq!(block.pass(&key(), &changed, "shop.test"), None);

        // Garbage
        let mut garbage = HeaderMap::new();
        garbage.insert(
            "cookie",
            format!("{}=%%%; x=1", block.cookie).parse().unwrap(),
        );
        assert_eq!(block.pass(&key(), &garbage, "shop.test"), None);
    }

    #[test]
    fn test_checking_a_pass_allocates_nothing() {
        let block = room(serde_json::json!({ "active": 1, "on_store_error": "wait" })).unwrap();
        let cookie = block
            .seal(&key(), PASS, "shop.test", block.limits.session, true)
            .unwrap();
        let (headers, key) = (sent(&cookie), key());
        crate::allocations::assert_no_allocations("checking a pass", || {
            block.pass(&key, &headers, "shop.test")
        });
    }
}
