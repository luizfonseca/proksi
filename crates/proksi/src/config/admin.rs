//! `admin { listen = "...", token "name" { ... } }`: the admin API (ADR 0004, ADR 0008)

use std::{collections::BTreeMap, net::SocketAddr};

use serde::{Deserialize, Serialize};

/// `admin { ... }`
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AdminConfig {
    /// The address of the admin API (`ip:port`)
    pub listen: String,
    /// `token "name" { secret = "...", access = "write" }`, by name. Without one, `listen` must
    /// be a loopback address.
    #[serde(default, rename = "token", skip_serializing)]
    pub tokens: BTreeMap<String, Token>,
    /// Record metrics (`/metrics` and live stats). Off, requests don't record anything.
    #[serde(default = "enabled_by_default")]
    pub metrics: bool,
    /// Another address serving only `/metrics` (`ip:port`), for scrapers that mustn't reach
    /// the rest of the API
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metrics_listen: Option<String>,
    /// Required in `Authorization: Bearer <token>` on `metrics_listen`, if set
    #[serde(default, skip_serializing)]
    pub metrics_token: Option<String>,
}

/// `token "name" { ... }`: a secret given in `Authorization: Bearer <secret>`, or to sign in
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Token {
    #[serde(skip_serializing)]
    pub secret: String,
    #[serde(default)]
    pub access: Access,
}

/// What a token can do: read, unless it says `access = "write"`
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Access {
    #[default]
    Read,
    Write,
}

fn enabled_by_default() -> bool {
    true
}

impl AdminConfig {
    /// The address to listen on, if the block is valid
    pub fn address(&self) -> Result<SocketAddr, String> {
        let address = self.listen.parse::<SocketAddr>().map_err(|_| {
            format!(
                "admin: `listen` must be an ip:port address, not {:?}",
                self.listen
            )
        })?;
        if let Some((name, _)) = self
            .tokens
            .iter()
            .find(|(_, token)| token.secret.is_empty())
        {
            return Err(format!("admin: token {name:?} has an empty secret"));
        }
        if self.tokens.is_empty() && !address.ip().is_loopback() {
            return Err(format!(
                "admin: without a `token` block, `listen` must be a loopback address (e.g. 127.0.0.1:7555), not {address}"
            ));
        }
        Ok(address)
    }

    /// The address of the metrics-only listener, if any
    pub fn metrics_address(&self) -> Result<Option<SocketAddr>, String> {
        let Some(listen) = &self.metrics_listen else {
            return Ok(None);
        };
        if !self.metrics {
            return Err("admin: metrics_listen needs metrics (metrics = false)".to_string());
        }
        listen.parse::<SocketAddr>().map(Some).map_err(|_| {
            format!("admin: `metrics_listen` must be an ip:port address, not {listen:?}")
        })
    }

    /// The token of the metrics-only listener, if set (an empty one is none)
    pub fn metrics_token(&self) -> Option<&str> {
        self.metrics_token
            .as_deref()
            .filter(|token| !token.is_empty())
    }
}

#[cfg(test)]
mod tests {
    use figment::providers::Format;

    use crate::config::hcl::HclFormat;

    use super::{Access, AdminConfig};

    fn admin(hcl: &str) -> AdminConfig {
        #[derive(serde::Deserialize)]
        struct Doc {
            admin: AdminConfig,
        }
        HclFormat::from_str::<Doc>(hcl).unwrap().admin
    }

    #[test]
    fn test_admin_block() {
        let admin = admin(
            "admin {\n  listen = \"0.0.0.0:7555\"\n  token \"grafana\" {\n    secret = \"a\"\n  }\n  token \"ops\" {\n    secret = \"b\"\n    access = \"write\"\n  }\n}\n",
        );

        assert_eq!(admin.tokens.len(), 2);
        assert_eq!(admin.tokens["grafana"].secret, "a");
        assert_eq!(admin.tokens["grafana"].access, Access::Read);
        assert_eq!(admin.tokens["ops"].access, Access::Write);
        assert!(admin.metrics);
        assert!(
            !self::admin("admin {\n  listen = \"127.0.0.1:7555\"\n  metrics = false\n}\n").metrics
        );
        assert_eq!(admin.address(), Ok("0.0.0.0:7555".parse().unwrap()));
    }

    #[test]
    fn test_without_a_token_only_on_loopback() {
        assert_eq!(
            admin("admin {\n  listen = \"127.0.0.1:7555\"\n}\n").address(),
            Ok("127.0.0.1:7555".parse().unwrap())
        );
        assert!(admin("admin {\n  listen = \"[::1]:7555\"\n}\n")
            .address()
            .is_ok());
        assert_eq!(
            admin("admin {\n  listen = \"0.0.0.0:7555\"\n}\n").address(),
            Err("admin: without a `token` block, `listen` must be a loopback address (e.g. 127.0.0.1:7555), not 0.0.0.0:7555".to_string())
        );
        assert_eq!(
            admin("admin {\n  listen = \"0.0.0.0:7555\"\n  token \"ops\" {\n    secret = \"\"\n  }\n}\n")
                .address(),
            Err("admin: token \"ops\" has an empty secret".to_string())
        );
    }

    #[test]
    fn test_tokens_are_blocks() {
        #[derive(Debug, serde::Deserialize)]
        #[allow(dead_code)]
        struct Doc {
            admin: AdminConfig,
        }
        // `token = "..."` is gone, and `access` is "read" or "write", nothing else
        for hcl in [
            "admin {\n  listen = \"127.0.0.1:7555\"\n  token = \"secret\"\n}\n",
            "admin {\n  listen = \"127.0.0.1:7555\"\n  token \"ops\" {\n    secret = \"a\"\n    access = \"rw\"\n  }\n}\n",
        ] {
            assert!(HclFormat::from_str::<Doc>(hcl).is_err(), "{hcl}");
        }
    }

    #[test]
    fn test_invalid_address() {
        assert_eq!(
            admin("admin {\n  listen = \"7555\"\n}\n").address(),
            Err("admin: `listen` must be an ip:port address, not \"7555\"".to_string())
        );
    }
}
