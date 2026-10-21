//! `ip_filter { allow = ["10.0.0.0/8"], deny = ["10.0.0.13"] }`: answers `403` to the clients
//! a location doesn't accept

use std::net::IpAddr;

use anyhow::Result;
use async_trait::async_trait;
use http::{header, StatusCode};
use crate::ip_set::{network, IpSet};
use pingora::{http::ResponseHeader, proxy::Session};

use crate::{respond::respond, RequestCtx, RoutePlugin};

use super::Middleware;

/// Clients (by IP address, behind `server.trusted_proxies` the one they forwarded) in `deny`
/// are refused, and when `allow` is set, so is every client not in it
pub(super) struct IpFilter {
    allow: IpSet,
    deny: IpSet,
}

impl IpFilter {
    pub(super) fn new(plugin: &RoutePlugin) -> Result<Self, String> {
        let settings: super::settings::IpFilter = super::settings::read("ip_filter", plugin)?;
        let networks = |entries: Option<Vec<String>>| -> Result<IpSet, String> {
            entries
                .unwrap_or_default()
                .iter()
                .map(|entry| {
                    network(entry).ok_or_else(|| {
                        format!("ip_filter: {entry:?} is not an address or a network")
                    })
                })
                .collect::<Result<Vec<_>, _>>()
                .map(IpSet::new)
        };
        let (allow, deny) = (networks(settings.allow)?, networks(settings.deny)?);
        if allow.is_empty() && deny.is_empty() {
            return Err("ip_filter: `allow` or `deny` is required".to_string());
        }
        Ok(Self { allow, deny })
    }

    /// IPv4 clients connected over IPv6 (`::ffff:10.0.0.1`) are matched as IPv4
    fn accepts(&self, client: &IpAddr) -> bool {
        !self.deny.contains(*client) && (self.allow.is_empty() || self.allow.contains(*client))
    }
}

#[async_trait]
impl Middleware for IpFilter {
    async fn request(&self, session: &mut Session, ctx: &mut dyn RequestCtx) -> Result<bool> {
        let Some(client) = ctx.client(session) else {
            return Ok(false);
        };
        if self.accepts(&client) {
            return Ok(false);
        }

        let mut response = ResponseHeader::build_no_case(StatusCode::FORBIDDEN, Some(1))?;
        response.insert_header(header::CONTENT_LENGTH, "0")?;
        respond(session, ctx, response).await?;
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use std::borrow::Cow;

    use crate::RoutePlugin;

    use super::IpFilter;

    fn filter(config: serde_json::Value) -> Result<IpFilter, String> {
        IpFilter::new(&RoutePlugin {
            name: Cow::Borrowed("ip_filter"),
            config: serde_json::from_value(config).unwrap(),
        })
    }

    fn accepts(filter: &IpFilter, client: &str) -> bool {
        filter.accepts(&client.parse().unwrap())
    }

    #[test]
    fn test_allow_and_deny() {
        let only = filter(serde_json::json!({
            "allow": ["10.0.0.0/8", "2001:db8::/32", "203.0.113.9"],
            "deny": ["10.0.0.13"],
        }))
        .unwrap();
        assert!(accepts(&only, "10.1.2.3"));
        assert!(accepts(&only, "203.0.113.9"));
        assert!(accepts(&only, "2001:db8::1"));
        assert!(!accepts(&only, "10.0.0.13"));
        assert!(!accepts(&only, "203.0.113.10"));
        // An IPv4 client over IPv6
        assert!(accepts(&only, "::ffff:10.1.2.3"));
        assert!(!accepts(&only, "::ffff:10.0.0.13"));

        let except = filter(serde_json::json!({ "deny": ["198.51.100.0/24"] })).unwrap();
        assert!(accepts(&except, "203.0.113.9"));
        assert!(!accepts(&except, "198.51.100.7"));
    }

    #[test]
    fn test_invalid_settings() {
        assert_eq!(
            filter(serde_json::json!({})).err().as_deref(),
            Some("ip_filter: `allow` or `deny` is required")
        );
        assert_eq!(
            filter(serde_json::json!({ "allow": ["10.0.0"] })).err().as_deref(),
            Some("ip_filter: \"10.0.0\" is not an address or a network")
        );
        assert!(filter(serde_json::json!({ "deny": "10.0.0.1" })).is_err());
        assert!(filter(serde_json::json!({ "deny": [1] })).is_err());
    }
}
