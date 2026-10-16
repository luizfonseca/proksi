//! `admin { listen = "...", token = "..." }`: the admin API (ADR 0004)

use std::net::SocketAddr;

use serde::{Deserialize, Serialize};

/// `admin { ... }`
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AdminConfig {
    /// The address of the admin API (`ip:port`)
    pub listen: String,
    /// Required in `Authorization: Bearer <token>`. Without one, `listen` must be a loopback
    /// address.
    #[serde(skip_serializing)]
    pub token: Option<String>,
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
        if self.token().is_none() && !address.ip().is_loopback() {
            return Err(format!(
                "admin: without a `token`, `listen` must be a loopback address (e.g. 127.0.0.1:7555), not {address}"
            ));
        }
        Ok(address)
    }

    /// The token, if set (an empty one is none)
    pub fn token(&self) -> Option<&str> {
        self.token.as_deref().filter(|token| !token.is_empty())
    }
}

#[cfg(test)]
mod tests {
    use figment::providers::Format;

    use crate::config::hcl::HclFormat;

    use super::AdminConfig;

    fn admin(hcl: &str) -> AdminConfig {
        #[derive(serde::Deserialize)]
        struct Doc {
            admin: AdminConfig,
        }
        HclFormat::from_str::<Doc>(hcl).unwrap().admin
    }

    #[test]
    fn test_admin_block() {
        let admin = admin("admin {\n  listen = \"0.0.0.0:7555\"\n  token = \"secret\"\n}\n");

        assert_eq!(admin.token.as_deref(), Some("secret"));
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
            Err("admin: without a `token`, `listen` must be a loopback address (e.g. 127.0.0.1:7555), not 0.0.0.0:7555".to_string())
        );
        // An empty token is no token
        assert!(admin("admin {\n  listen = \"0.0.0.0:7555\"\n  token = \"\"\n}\n")
            .address()
            .is_err());
    }

    #[test]
    fn test_invalid_address() {
        assert_eq!(
            admin("admin {\n  listen = \"7555\"\n}\n").address(),
            Err("admin: `listen` must be an ip:port address, not \"7555\"".to_string())
        );
    }
}
