//! Who calls the admin API (ADR 0008): `token` blocks, given as `Authorization: Bearer`, or
//! exchanged by the UI for a session cookie

use std::{
    collections::{BTreeMap, HashMap},
    fmt::Write,
    sync::{Mutex, PoisonError},
    time::{Duration, Instant},
};

use crate::config::admin::{Access, Token};

/// The session cookie's name
const COOKIE: &str = "proksi_session";

/// A session ends after this long without a request
const IDLE: Duration = Duration::from_hours(12);

/// Who made a request (`GET /api/v1/session`)
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct Caller {
    /// The token's name (none without `token` blocks)
    pub name: Option<String>,
    pub access: Access,
    /// Signed in with a session cookie (rather than a bearer token)
    #[serde(skip)]
    pub session: bool,
}

struct Session {
    name: String,
    access: Access,
    last_seen: Instant,
}

pub struct Tokens {
    tokens: BTreeMap<String, Token>,
    /// By id: 32 random bytes, in hex. Kept in memory: they end when Proksi restarts.
    sessions: Mutex<HashMap<String, Session>>,
}

impl Tokens {
    pub fn new(tokens: BTreeMap<String, Token>) -> Self {
        Self {
            tokens,
            sessions: Mutex::new(HashMap::new()),
        }
    }

    /// Without tokens, anyone reaching the (loopback) listener can read and write
    pub fn open(&self) -> bool {
        self.tokens.is_empty()
    }

    /// The token with this secret, compared in constant time with every token
    fn token(&self, secret: &str) -> Option<(&String, Access)> {
        let mut found = None;
        for (name, token) in &self.tokens {
            let same = secret.len() == token.secret.len()
                && openssl::memcmp::eq(secret.as_bytes(), token.secret.as_bytes());
            if same {
                found = Some((name, token.access));
            }
        }
        found
    }

    /// The token named `name`, if this is its secret. An unknown name costs the same comparison,
    /// so the time it takes doesn't tell which names there are.
    fn named(&self, name: &str, secret: &str) -> Option<(&String, Access)> {
        let Some((name, token)) = self.tokens.get_key_value(name) else {
            let _ = openssl::memcmp::eq(secret.as_bytes(), secret.as_bytes());
            return None;
        };
        let same = secret.len() == token.secret.len()
            && openssl::memcmp::eq(secret.as_bytes(), token.secret.as_bytes());
        same.then_some((name, token.access))
    }

    /// Who's calling, from `Authorization: Bearer <secret>` or the session cookie
    pub fn caller(&self, authorization: Option<&str>, cookie: Option<&str>) -> Option<Caller> {
        if self.open() {
            return Some(Caller {
                name: None,
                access: Access::Write,
                session: false,
            });
        }
        if let Some(secret) = authorization.and_then(|value| value.strip_prefix("Bearer ")) {
            return self.token(secret).map(|(name, access)| Caller {
                name: Some(name.clone()),
                access,
                session: false,
            });
        }
        let id = session_id(cookie?)?;
        let mut sessions = self.sessions.lock().unwrap_or_else(PoisonError::into_inner);
        let session = sessions.get_mut(id)?;
        if session.last_seen.elapsed() > IDLE {
            sessions.remove(id);
            return None;
        }
        session.last_seen = Instant::now();
        Some(Caller {
            name: Some(session.name.clone()),
            access: session.access,
            session: true,
        })
    }

    /// A new session for the token named `name` with this secret (without a name, any token's):
    /// its id and caller
    pub fn sign_in(&self, name: Option<&str>, secret: &str) -> Option<(String, Caller)> {
        let (name, access) = match name {
            Some(name) => self.named(name, secret)?,
            None => self.token(secret)?,
        };
        let mut bytes = [0u8; 32];
        openssl::rand::rand_bytes(&mut bytes).ok()?;
        let id = bytes
            .iter()
            .fold(String::with_capacity(64), |mut id, byte| {
                let _ = write!(id, "{byte:02x}");
                id
            });

        let mut sessions = self.sessions.lock().unwrap_or_else(PoisonError::into_inner);
        sessions.retain(|_, session| session.last_seen.elapsed() <= IDLE);
        sessions.insert(
            id.clone(),
            Session {
                name: name.clone(),
                access,
                last_seen: Instant::now(),
            },
        );
        let caller = Caller {
            name: Some(name.clone()),
            access,
            session: true,
        };
        Some((id, caller))
    }

    /// Ends the session in the cookie, if any
    pub fn sign_out(&self, cookie: Option<&str>) {
        if let Some(id) = cookie.and_then(session_id) {
            let mut sessions = self.sessions.lock().unwrap_or_else(PoisonError::into_inner);
            sessions.remove(id);
        }
    }
}

/// The session's id in a `Cookie` header
fn session_id(cookie: &str) -> Option<&str> {
    cookie
        .split(';')
        .find_map(|pair| pair.trim().strip_prefix(COOKIE)?.strip_prefix('='))
        .filter(|id| !id.is_empty())
}

/// `Set-Cookie` for a session (`None`: ending it). `Secure` unless the listener is reached on a
/// loopback address, over plain HTTP.
pub fn set_cookie(id: Option<&str>, host: Option<&str>) -> String {
    let secure = if loopback(host.unwrap_or_default()) {
        ""
    } else {
        "; Secure"
    };
    match id {
        Some(id) => format!("{COOKIE}={id}; Path=/; HttpOnly; SameSite=Strict{secure}"),
        None => format!("{COOKIE}=; Path=/; HttpOnly; SameSite=Strict; Max-Age=0{secure}"),
    }
}

/// Whether a `Host` header names a loopback address
fn loopback(host: &str) -> bool {
    let name = match host.strip_prefix('[') {
        Some(rest) => rest.split(']').next().unwrap_or_default(),
        None => host.split(':').next().unwrap_or_default(),
    };
    name.eq_ignore_ascii_case("localhost")
        || name
            .parse::<std::net::IpAddr>()
            .is_ok_and(|ip| ip.is_loopback())
}

/// Whether a change with a session comes from the UI's own pages: its `Origin` is the address
/// the request was sent to (`Host`), or one of `trusted` (behind a proxy that changes `Host`)
pub fn allowed_origin(origin: Option<&str>, host: Option<&str>, trusted: &[String]) -> bool {
    same_origin(origin, host)
        || origin.is_some_and(|origin| trusted.iter().any(|one| one.eq_ignore_ascii_case(origin)))
}

/// Whether `Origin` names the address the request was sent to (`Host`)
pub fn same_origin(origin: Option<&str>, host: Option<&str>) -> bool {
    let (Some(origin), Some(host)) = (origin, host) else {
        return false;
    };
    origin
        .strip_prefix("http://")
        .or_else(|| origin.strip_prefix("https://"))
        .is_some_and(|authority| authority.eq_ignore_ascii_case(host))
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use crate::config::admin::{Access, Token};

    use super::{allowed_origin, same_origin, set_cookie, Tokens};

    fn tokens() -> Tokens {
        let token = |secret: &str, access| Token {
            secret: secret.to_string(),
            access,
        };
        Tokens::new(BTreeMap::from([
            ("grafana".to_string(), token("read-secret", Access::Read)),
            ("ops".to_string(), token("write-secret", Access::Write)),
        ]))
    }

    #[test]
    fn test_bearer_tokens() {
        let tokens = tokens();

        let ops = tokens.caller(Some("Bearer write-secret"), None).unwrap();
        assert_eq!(ops.name.as_deref(), Some("ops"));
        assert_eq!(ops.access, Access::Write);
        assert!(!ops.session);
        let grafana = tokens.caller(Some("Bearer read-secret"), None).unwrap();
        assert_eq!(grafana.access, Access::Read);
        for authorization in [
            None,
            Some("Bearer wrong"),
            Some("write-secret"),
            Some("Basic write-secret"),
        ] {
            assert_eq!(
                tokens.caller(authorization, None),
                None,
                "{authorization:?}"
            );
        }
    }

    #[test]
    fn test_without_tokens_anyone_writes() {
        let tokens = Tokens::new(BTreeMap::new());

        let caller = tokens.caller(None, None).unwrap();
        assert_eq!(caller.access, Access::Write);
        assert_eq!(caller.name, None);
    }

    #[test]
    fn test_signing_in_by_name() {
        let tokens = tokens();

        let (_, caller) = tokens.sign_in(Some("grafana"), "read-secret").unwrap();
        assert_eq!(caller.name.as_deref(), Some("grafana"));
        assert_eq!(caller.access, Access::Read);
        // Only that token's secret
        assert!(tokens.sign_in(Some("ops"), "read-secret").is_none());
        assert!(tokens.sign_in(Some("nobody"), "read-secret").is_none());
        assert!(tokens.sign_in(Some("grafana"), "wrong").is_none());
        // Without a name, any token's
        let (_, caller) = tokens.sign_in(None, "write-secret").unwrap();
        assert_eq!(caller.name.as_deref(), Some("ops"));
    }

    #[test]
    fn test_sessions() {
        let tokens = tokens();
        assert!(tokens.sign_in(None, "wrong").is_none());

        let (id, caller) = tokens.sign_in(None, "read-secret").unwrap();
        assert_eq!(id.len(), 64);
        assert_eq!(caller.name.as_deref(), Some("grafana"));
        let cookie = format!("theme=dark; proksi_session={id}");
        let signed_in = tokens.caller(None, Some(&cookie)).unwrap();
        assert!(signed_in.session);
        assert_eq!(signed_in.access, Access::Read);
        assert_eq!(tokens.caller(None, Some("proksi_session=nope")), None);
        assert_eq!(tokens.caller(None, Some("proksi_session=")), None);

        tokens.sign_out(Some(&cookie));
        assert_eq!(tokens.caller(None, Some(&cookie)), None);
    }

    #[test]
    fn test_cookies() {
        assert_eq!(
            set_cookie(Some("abc"), Some("127.0.0.1:7555")),
            "proksi_session=abc; Path=/; HttpOnly; SameSite=Strict"
        );
        for host in ["localhost:5173", "[::1]:7555"] {
            assert!(
                !set_cookie(Some("abc"), Some(host)).contains("Secure"),
                "{host}"
            );
        }
        for host in [Some("admin.example.com"), Some("10.0.0.1:7555"), None] {
            assert!(
                set_cookie(Some("abc"), host).ends_with("; Secure"),
                "{host:?}"
            );
        }
        assert_eq!(
            set_cookie(None, Some("127.0.0.1:7555")),
            "proksi_session=; Path=/; HttpOnly; SameSite=Strict; Max-Age=0"
        );
    }

    #[test]
    fn test_trusted_origins() {
        let trusted = vec!["https://host.tailnet.ts.net:9200".to_string()];
        // Behind a proxy sending its own Host
        let host = Some("127.0.0.1:7555");
        assert!(allowed_origin(Some("https://host.tailnet.ts.net:9200"), host, &trusted));
        assert!(allowed_origin(Some("https://HOST.tailnet.ts.net:9200"), host, &trusted));
        for origin in [Some("https://host.tailnet.ts.net"), Some("http://host.tailnet.ts.net:9200"), Some("null"), None] {
            assert!(!allowed_origin(origin, host, &trusted), "{origin:?}");
        }
        // The listener's own address still is
        assert!(allowed_origin(Some("http://127.0.0.1:7555"), host, &[]));
    }

    #[test]
    fn test_same_origin() {
        assert!(same_origin(
            Some("http://127.0.0.1:7555"),
            Some("127.0.0.1:7555")
        ));
        assert!(same_origin(
            Some("https://Admin.example.com"),
            Some("admin.example.com")
        ));
        for (origin, host) in [
            (Some("http://evil.test"), Some("127.0.0.1:7555")),
            (Some("http://127.0.0.1:7556"), Some("127.0.0.1:7555")),
            (Some("null"), Some("127.0.0.1:7555")),
            (None, Some("127.0.0.1:7555")),
            (Some("http://127.0.0.1:7555"), None),
        ] {
            assert!(!same_origin(origin, host), "{origin:?} {host:?}");
        }
    }
}
