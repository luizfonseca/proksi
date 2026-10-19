//! `GET /api/v1/server`: the settings read when Proksi starts, with their defaults filled in.
//! Each field is listed here, so a new setting (or a secret) is never shown by accident.

use std::path::PathBuf;

use serde::Serialize;

use crate::config::{admin::Access, server_tls::TlsVersion, ClientTimeouts, Config, StoreType};

#[derive(Debug, Clone, Serialize)]
pub struct Settings {
    pub service_name: String,
    /// Threads of the HTTPS proxy
    pub worker_threads: Option<usize>,
    pub config_path: Option<String>,
    pub server: Listening,
    pub store: StoreSettings,
    pub cache: Cache,
    pub lets_encrypt: LetsEncrypt,
    pub logging: Logging,
    pub auto_reload: AutoReload,
    pub docker: Docker,
    pub kubernetes: Kubernetes,
    pub admin: Option<Admin>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Listening {
    pub https_address: String,
    pub http_address: String,
    pub trusted_proxies: Vec<String>,
    pub shutdown_timeout_secs: u64,
    pub work_stealing: bool,
    pub timeouts: ClientTimeouts,
    /// The lowest TLS version accepted
    pub tls_min_version: TlsVersion,
    /// Empty: OpenSSL's defaults
    pub tls_ciphers: Vec<String>,
    /// `server { rule "..." {} }`, by name
    pub rules: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct StoreSettings {
    #[serde(rename = "type")]
    pub store_type: String,
    /// Where it is, without credentials
    pub address: Option<String>,
    pub sync_interval_secs: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct Cache {
    pub memory_limit_mb: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct LetsEncrypt {
    pub enabled: bool,
    pub staging: bool,
    pub email: String,
    pub renew_interval_secs: u64,
    /// Where certificates are kept
    pub path: PathBuf,
}

#[derive(Debug, Clone, Serialize)]
pub struct Logging {
    pub enabled: bool,
    pub level: String,
    pub format: String,
    pub access_logs: bool,
    pub error_logs: bool,
    pub path: Option<PathBuf>,
    pub rotation: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct AutoReload {
    pub enabled: bool,
    pub interval_secs: u64,
    pub paths: Vec<PathBuf>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Docker {
    pub enabled: bool,
    pub mode: String,
    pub endpoint: Option<String>,
    pub interval_secs: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct Kubernetes {
    pub enabled: bool,
    pub ingress_class: String,
    /// Empty: every namespace
    pub namespaces: Vec<String>,
    pub publish_service: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Admin {
    pub listen: String,
    pub metrics: bool,
    pub metrics_listen: Option<String>,
    /// Names and access, never secrets
    pub tokens: Vec<TokenInfo>,
    pub metrics_token: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct TokenInfo {
    pub name: String,
    pub access: Access,
}

impl Settings {
    pub fn new(config: &Config) -> Self {
        let server = &config.server;
        let store = &config.store;
        let url = match store.store_type {
            StoreType::Redis => store.redis_url.as_deref(),
            StoreType::Sqlite | StoreType::Postgres => store.database_url.as_deref(),
            StoreType::Memory => None,
        };
        Self {
            service_name: config.service_name.to_string(),
            worker_threads: config.worker_threads,
            config_path: config.config_path.as_ref().map(ToString::to_string),
            server: Listening {
                https_address: server
                    .https_address
                    .as_deref()
                    .unwrap_or("0.0.0.0:443")
                    .to_string(),
                http_address: server
                    .http_address
                    .as_deref()
                    .unwrap_or("0.0.0.0:80")
                    .to_string(),
                trusted_proxies: server.trusted_proxies.clone(),
                shutdown_timeout_secs: server.shutdown_timeout().as_secs(),
                work_stealing: server.work_stealing(),
                timeouts: server.timeouts,
                tls_min_version: server.tls.min_version,
                tls_ciphers: server.tls.ciphers.clone(),
                rules: server
                    .rule
                    .iter()
                    .filter_map(|rule| rule.0.get("name")?.as_str().map(ToString::to_string))
                    .collect(),
            },
            store: StoreSettings {
                store_type: written(&store.store_type),
                address: url.map(without_credentials),
                sync_interval_secs: store.sync_interval_secs,
            },
            cache: Cache {
                memory_limit_mb: config.cache.memory_limit_mb,
            },
            lets_encrypt: LetsEncrypt {
                enabled: config.lets_encrypt.enabled.unwrap_or(true),
                staging: config.lets_encrypt.staging.unwrap_or(true),
                email: config.lets_encrypt.email.to_string(),
                renew_interval_secs: config.lets_encrypt.renew_interval_secs.unwrap_or(84_600),
                path: config.paths.lets_encrypt.clone(),
            },
            logging: Logging {
                enabled: config.logging.enabled,
                level: written(&config.logging.level),
                format: written(&config.logging.format),
                access_logs: config.logging.access_logs_enabled,
                error_logs: config.logging.error_logs_enabled,
                path: config.logging.path.clone(),
                rotation: written(&config.logging.rotation),
            },
            auto_reload: AutoReload {
                enabled: config.auto_reload.enabled.unwrap_or(false),
                interval_secs: config.auto_reload.interval_secs.unwrap_or(30),
                paths: config.auto_reload.paths.clone(),
            },
            docker: Docker {
                enabled: config.docker.enabled.unwrap_or(false),
                mode: written(&config.docker.mode),
                endpoint: config.docker.endpoint.as_ref().map(ToString::to_string),
                interval_secs: config.docker.interval_secs.unwrap_or(15),
            },
            kubernetes: Kubernetes {
                enabled: config.kubernetes.enabled,
                ingress_class: config.kubernetes.ingress_class.clone(),
                namespaces: config.kubernetes.namespaces.clone(),
                publish_service: config.kubernetes.publish_service.clone(),
            },
            admin: config.admin.as_ref().map(|admin| Admin {
                listen: admin.listen.clone(),
                metrics: admin.metrics,
                metrics_listen: admin.metrics_listen.clone(),
                tokens: admin
                    .tokens
                    .iter()
                    .map(|(name, token)| TokenInfo {
                        name: name.clone(),
                        access: token.access,
                    })
                    .collect(),
                metrics_token: admin.metrics_token().is_some(),
            }),
        }
    }
}

/// A setting as written in the configuration (`Redis` → `redis`)
fn written(value: &impl std::fmt::Debug) -> String {
    format!("{value:?}").to_lowercase()
}

/// A store's URL without its user and password (`redis://:secret@host:6379/0` →
/// `redis://host:6379/0`), or its query (which can hold them too)
fn without_credentials(url: &str) -> String {
    let url = url.split('?').next().unwrap_or_default();
    match url.split_once("://") {
        Some((scheme, rest)) => {
            let rest = rest.rsplit_once('@').map_or(rest, |(_, host)| host);
            format!("{scheme}://{rest}")
        }
        None => url
            .rsplit_once('@')
            .map_or(url, |(_, path)| path)
            .to_string(),
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use crate::config::{
        admin::{Access, AdminConfig, Token},
        Config, StoreType,
    };

    use super::{without_credentials, Settings};

    #[test]
    fn test_no_credentials() {
        assert_eq!(
            without_credentials("redis://:secret@redis:6379/0"),
            "redis://redis:6379/0"
        );
        assert_eq!(
            without_credentials("postgres://user:p@ss@db/proksi?password=x"),
            "postgres://db/proksi"
        );
        assert_eq!(
            without_credentials("sqlite://proksi.db"),
            "sqlite://proksi.db"
        );
    }

    #[test]
    fn test_no_secrets() {
        let mut config = Config::default();
        config.store.store_type = StoreType::Redis;
        config.store.redis_url = Some("redis://:store-secret@redis:6379".to_string());
        config.admin = Some(AdminConfig {
            listen: "127.0.0.1:7555".to_string(),
            tokens: BTreeMap::from([(
                "ops".to_string(),
                Token {
                    secret: "token-secret".to_string(),
                    access: Access::Write,
                },
            )]),
            metrics: true,
            metrics_listen: None,
            metrics_token: Some("metrics-secret".to_string()),
        });

        let server = serde_json::to_string(&Settings::new(&config)).unwrap();

        assert!(
            server.contains(r#""tokens":[{"name":"ops","access":"write"}]"#),
            "{server}"
        );
        assert!(
            server.contains(r#""address":"redis://redis:6379""#),
            "{server}"
        );
        assert!(
            server.contains(r#""https_address":"0.0.0.0:443""#),
            "{server}"
        );
        for secret in ["store-secret", "token-secret", "metrics-secret"] {
            assert!(!server.contains(secret), "{server}");
        }
    }
}
