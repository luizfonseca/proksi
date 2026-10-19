//! `ip_filter { allow = ["10.0.0.0/8"], deny = ["10.0.0.13"] }`: answers `403` to the clients
//! a location doesn't accept

use std::net::IpAddr;

use anyhow::Result;
use async_trait::async_trait;
use http::{header, StatusCode};
use ipnet::IpNet;
use pingora::{http::ResponseHeader, proxy::Session};

use crate::{
    config::RoutePlugin,
    proxy_server::{https_proxy::RouterContext, respond::respond},
};

use super::Middleware;

/// Clients (by IP address, behind `server.trusted_proxies` the one they forwarded) in `deny`
/// are refused, and when `allow` is set, so is every client not in it
pub(super) struct IpFilter {
    allow: Vec<IpNet>,
    deny: Vec<IpNet>,
}

impl IpFilter {
    pub(super) fn new(plugin: &RoutePlugin) -> Result<Self, String> {
        let settings: super::settings::IpFilter = super::settings::read("ip_filter", plugin)?;
        let networks = |entries: Option<Vec<String>>| -> Result<Vec<IpNet>, String> {
            entries
                .unwrap_or_default()
                .iter()
                .map(|entry| {
                    entry
                        .parse::<IpNet>()
                        .or_else(|_| entry.parse::<IpAddr>().map(IpNet::from))
                        .map_err(|_| {
                            format!("ip_filter: {entry:?} is not an address or a network")
                        })
                })
                .collect()
        };
        let (allow, deny) = (networks(settings.allow)?, networks(settings.deny)?);
        if allow.is_empty() && deny.is_empty() {
            return Err("ip_filter: `allow` or `deny` is required".to_string());
        }
        Ok(Self { allow, deny })
    }

    fn accepts(&self, client: &IpAddr) -> bool {
        // IPv4 clients connected over IPv6 (`::ffff:10.0.0.1`) are matched as IPv4
        let client = match client {
            IpAddr::V6(v6) => v6.to_ipv4_mapped().map_or(*client, IpAddr::V4),
            IpAddr::V4(_) => *client,
        };
        let listed = |networks: &[IpNet]| networks.iter().any(|network| network.contains(&client));
        !listed(&self.deny) && (self.allow.is_empty() || listed(&self.allow))
    }
}

#[async_trait]
impl Middleware for IpFilter {
    async fn request(&self, session: &mut Session, ctx: &mut RouterContext) -> Result<bool> {
        let Some(client) = crate::proxy_server::client::of_session(session) else {
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

    use crate::config::RoutePlugin;

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
