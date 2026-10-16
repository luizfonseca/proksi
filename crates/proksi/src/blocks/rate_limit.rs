//! `rate_limit { requests = 120, per = "1m" }`: answers `429` to clients over the limit

use std::{net::IpAddr, time::Duration};

use anyhow::Result;
use async_trait::async_trait;
use http::{header, StatusCode};
use pingora::{http::ResponseHeader, proxy::Session};
use pingora_limits::rate::Rate;

use crate::{config::RoutePlugin, proxy_server::https_proxy::RouterContext};

use super::Middleware;

const INVALID_PER: &str = "rate_limit: `per` must be a duration such as \"10s\", \"1m\" or \"1h\"";

/// Counts the requests of each client (by IP address) in windows of `per`. Counts use a
/// fixed amount of memory and are estimates, which can only be higher than the real count.
pub(super) struct RateLimit {
    requests: isize,
    per: Duration,
    counts: Rate,
}

impl RateLimit {
    pub(super) fn new(plugin: &RoutePlugin) -> Result<Self, String> {
        let setting = |key: &str| plugin.config.as_ref().and_then(|config| config.get(key));

        let requests = setting("requests")
            .and_then(serde_json::Value::as_u64)
            .filter(|requests| *requests > 0)
            .and_then(|requests| isize::try_from(requests).ok())
            .ok_or("rate_limit: `requests` must be a number greater than 0")?;
        let per = setting("per")
            .and_then(serde_json::Value::as_str)
            .and_then(parse_duration)
            .ok_or(INVALID_PER)?;

        Ok(Self {
            requests,
            per,
            counts: Rate::new(per),
        })
    }

    /// Counts a request of `client`, returning whether it's over the limit
    fn exceeded(&self, client: &IpAddr) -> bool {
        self.counts.observe(client, 1) > self.requests
    }
}

#[async_trait]
impl Middleware for RateLimit {
    async fn request(&self, session: &mut Session, _ctx: &mut RouterContext) -> Result<bool> {
        let Some(client) = session
            .client_addr()
            .and_then(|addr| addr.as_inet())
            .map(std::net::SocketAddr::ip)
        else {
            return Ok(false);
        };
        if !self.exceeded(&client) {
            return Ok(false);
        }

        let mut response = ResponseHeader::build_no_case(StatusCode::TOO_MANY_REQUESTS, Some(1))?;
        response.insert_header(header::RETRY_AFTER, self.per.as_secs().to_string())?;
        response.insert_header(header::CONTENT_LENGTH, "0")?;
        session
            .write_response_header(Box::new(response), true)
            .await?;
        Ok(true)
    }
}

/// A duration like `10s`, `1m` or `1h`
fn parse_duration(value: &str) -> Option<Duration> {
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

    use crate::config::RoutePlugin;

    use super::{parse_duration, RateLimit};

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

        assert!(!limit.exceeded(&client));
        assert!(!limit.exceeded(&client));
        assert!(limit.exceeded(&client));
        // Each client has its own count
        assert!(!limit.exceeded(&other));
    }

    #[test]
    fn test_invalid_settings() {
        for (config, expected) in [
            (
                serde_json::json!({ "per": "1m" }),
                "rate_limit: `requests` must be a number greater than 0",
            ),
            (
                serde_json::json!({ "requests": 0, "per": "1m" }),
                "rate_limit: `requests` must be a number greater than 0",
            ),
            (
                serde_json::json!({ "requests": 10 }),
                "rate_limit: `per` must be a duration such as \"10s\", \"1m\" or \"1h\"",
            ),
            (
                serde_json::json!({ "requests": 10, "per": "1x" }),
                "rate_limit: `per` must be a duration such as \"10s\", \"1m\" or \"1h\"",
            ),
        ] {
            assert_eq!(
                RateLimit::new(&plugin(config)).err().as_deref(),
                Some(expected)
            );
        }
    }
}
