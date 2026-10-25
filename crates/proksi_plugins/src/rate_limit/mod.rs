//! `rate_limit { requests = 120, per = "1m", burst = 30 }`: answers `429` to clients over the
//! limit. Clients are decided with GCRA (see `gcra`): `requests + burst` at once, then
//! `requests` per `per`, spread evenly.

use std::{fmt, net::IpAddr, time::Duration};

use anyhow::Result;
use async_trait::async_trait;
use http::{StatusCode, header};
use pingora::{http::ResponseHeader, proxy::Session};

use crate::{RequestCtx, RoutePlugin, respond::respond};

use super::Middleware;
use gcra::Gcra;

mod gcra;

/// Why a `rate_limit` block can't be used
#[derive(Debug, Clone, PartialEq)]
pub(super) enum RateLimitError {
    /// Its settings can't be read (missing, mistyped or unknown), as `settings::read` says
    Settings(String),
    /// `requests` is 0
    NoRequests,
    /// `per` isn't a duration
    InvalidPer,
    /// `burst` is 0
    NoBurst,
}

impl fmt::Display for RateLimitError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RateLimitError::Settings(err) => write!(f, "{err}"),
            RateLimitError::NoRequests => {
                write!(f, "rate_limit: `requests` must be a number greater than 0")
            }
            RateLimitError::InvalidPer => write!(
                f,
                "rate_limit: `per` must be a duration such as \"10s\", \"1m\" or \"1h\""
            ),
            RateLimitError::NoBurst => {
                write!(f, "rate_limit: `burst` must be a number greater than 0")
            }
        }
    }
}

impl std::error::Error for RateLimitError {}

/// The requests of each client (by IP address), in a fixed amount of memory: clients sharing a
/// slot can only be limited early, never late
pub(super) struct RateLimit {
    limits: Gcra,
}

impl RateLimit {
    pub(super) fn new(plugin: &RoutePlugin) -> Result<Self, RateLimitError> {
        let settings: super::settings::RateLimit =
            super::settings::read("rate_limit", plugin).map_err(RateLimitError::Settings)?;
        let requests = Some(settings.requests)
            .filter(|requests| *requests > 0)
            .ok_or(RateLimitError::NoRequests)?;
        let per = parse_duration(&settings.per).ok_or(RateLimitError::InvalidPer)?;
        if settings.burst == Some(0) {
            return Err(RateLimitError::NoBurst);
        }

        Ok(Self {
            limits: Gcra::new(requests, per, settings.burst.unwrap_or(0)),
        })
    }

    /// Counts a request of `client` if it's let through, or says how long until one would be
    fn exceeded(&self, client: &IpAddr) -> Option<Duration> {
        self.limits.check(client, self.limits.now()).err()
    }
}

#[async_trait]
impl Middleware for RateLimit {
    async fn request(&self, session: &mut Session, ctx: &mut dyn RequestCtx) -> Result<bool> {
        let Some(client) = ctx.client(session) else {
            return Ok(false);
        };
        let Some(wait) = self.exceeded(&client) else {
            return Ok(false);
        };

        let mut response = ResponseHeader::build_no_case(StatusCode::TOO_MANY_REQUESTS, Some(1))?;
        response.insert_header(header::RETRY_AFTER, retry_after(wait).to_string())?;
        response.insert_header(header::CONTENT_LENGTH, "0")?;
        respond(session, ctx, response).await?;
        Ok(true)
    }
}

/// `Retry-After`: whole seconds, rounded up, at least 1
fn retry_after(wait: Duration) -> u64 {
    wait.as_secs()
        .saturating_add(u64::from(wait.subsec_nanos() > 0))
        .max(1)
}

/// A duration like `10s`, `1m` or `1h`
pub(crate) fn parse_duration(value: &str) -> Option<Duration> {
    let unit = value.chars().last()?;
    let amount = value[..value.len() - unit.len_utf8()].parse::<u64>().ok()?;
    let seconds = match unit {
        's' => amount,
        'm' => amount.checked_mul(60)?,
        'h' => amount.checked_mul(3600)?,
        _ => return None,
    };
    (seconds > 0).then(|| Duration::from_secs(seconds))
}

#[cfg(test)]
mod tests {
    use std::{net::IpAddr, time::Duration};

    use crate::RoutePlugin;

    use super::{RateLimit, RateLimitError, parse_duration, retry_after};

    fn plugin(config: serde_json::Value) -> RoutePlugin {
        serde_json::from_value(serde_json::json!({ "name": "rate_limit", "config": config }))
            .unwrap()
    }

    #[test]
    fn test_parse_duration() {
        assert_eq!(parse_duration("1s"), Some(Duration::from_secs(1)));
        assert_eq!(parse_duration("10s"), Some(Duration::from_secs(10)));
        assert_eq!(parse_duration("1m"), Some(Duration::from_secs(60)));
        assert_eq!(parse_duration("2h"), Some(Duration::from_secs(7200)));
        for invalid in ["", "1", "s", "0s", "1x", "-1s", "1.5m"] {
            assert_eq!(parse_duration(invalid), None, "{invalid}");
        }
    }

    #[test]
    fn test_requests_over_the_limit_are_exceeded() {
        let limit =
            RateLimit::new(&plugin(serde_json::json!({ "requests": 2, "per": "1m" }))).unwrap();
        let client: IpAddr = "10.0.0.1".parse().unwrap();
        let other: IpAddr = "10.0.0.2".parse().unwrap();

        assert!(limit.exceeded(&client).is_none());
        assert!(limit.exceeded(&client).is_none());
        assert!(limit.exceeded(&client).is_some());
        // Each client has its own count
        assert!(limit.exceeded(&other).is_none());
    }

    #[test]
    fn test_burst_lets_more_through_at_once() {
        let limit = RateLimit::new(&plugin(
            serde_json::json!({ "requests": 2, "per": "1m", "burst": 1 }),
        ))
        .unwrap();
        let client: IpAddr = "10.0.0.1".parse().unwrap();
        assert!((0..3).all(|_| limit.exceeded(&client).is_none()));
        assert!(limit.exceeded(&client).is_some());
    }

    #[test]
    fn test_retry_after_is_at_least_one_second() {
        // 1000 per second: the next request is 1ms away, which is still `Retry-After: 1`
        let limit = RateLimit::new(&plugin(
            serde_json::json!({ "requests": 1000, "per": "1s" }),
        ))
        .unwrap();
        let client: IpAddr = "10.0.0.1".parse().unwrap();
        for _ in 0..1000 {
            limit.exceeded(&client);
        }
        assert_eq!(limit.exceeded(&client).map(retry_after), Some(1));
    }

    #[test]
    fn test_invalid_settings() {
        for (config, expected) in [
            (
                serde_json::json!({ "per": "1m" }),
                RateLimitError::Settings("rate_limit: `requests` is required".to_string()),
            ),
            (
                serde_json::json!({ "requests": 0, "per": "1m" }),
                RateLimitError::NoRequests,
            ),
            (
                serde_json::json!({ "requests": 10, "per": "1x" }),
                RateLimitError::InvalidPer,
            ),
            (
                serde_json::json!({ "requests": 10, "per": "1m", "burst": 0 }),
                RateLimitError::NoBurst,
            ),
        ] {
            assert_eq!(RateLimit::new(&plugin(config)).err(), Some(expected));
        }
    }

    #[test]
    fn test_errors_say_what_is_wrong() {
        for (error, message) in [
            (
                RateLimitError::Settings("rate_limit: `per` is required".to_string()),
                "rate_limit: `per` is required",
            ),
            (
                RateLimitError::NoRequests,
                "rate_limit: `requests` must be a number greater than 0",
            ),
            (
                RateLimitError::InvalidPer,
                "rate_limit: `per` must be a duration such as \"10s\", \"1m\" or \"1h\"",
            ),
            (
                RateLimitError::NoBurst,
                "rate_limit: `burst` must be a number greater than 0",
            ),
        ] {
            assert_eq!(error.to_string(), message);
        }
    }
}
