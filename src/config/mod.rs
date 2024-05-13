use figment::{
    providers::{Env, Format, Toml, Yaml},
    Figment, Provider,
};
use serde::{Deserialize, Deserializer, Serialize};

#[derive(Debug, Serialize, Deserialize)]
pub struct ConfigPath {
    // TLS
    /// Path to the certificates directory (where the certificates are stored)
    pub tls_certificates: String,
    /// Path to the challenges directory (where the challenges are stored)
    pub tls_challenges: String,
    /// Path to the order file for let's encrypt (JSON with a URL)
    pub tls_order: String,
    /// Path to the account credentials file for let's encrypt
    pub tls_account_credentials: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ConfigRouteHeaderAdd {
    /// The name of the header
    pub name: String,

    /// The value of the header
    pub value: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ConfigRouteHeaderRemove {
    /// The name of the header to remove (ex.: "Server")
    pub name: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ConfigRouteHeader {
    /// The name of the header
    pub add: Vec<ConfigRouteHeaderAdd>,

    /// The value of the header
    pub remove: Vec<ConfigRouteHeaderRemove>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ConfigRouteUpstream {
    /// The TCP address of the upstream (ex. 10.0.0.1/24 etc)
    pub ip: String,

    /// The port of the upstream (ex: 3000, 5000, etc.)
    pub port: i16,

    /// The network of the upstream (ex: 'public', 'shared') -- useful for docker discovery
    pub network: Option<String>,

    /// Optional: The weight of the upstream (ex: 1, 2, 3, etc.) --
    /// used for weight-based load balancing.
    pub weight: Option<i8>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ConfigRoute {
    /// The hostname that the proxy will accept
    /// requests for the upstreams in the route.
    /// (ex: 'example.com', 'www.example.com', etc.)
    ///
    /// This is the host header that the proxy will match and will
    /// also be used to create the certificate for the domain when `letsencrypt` is enabled.
    pub host: String,

    pub headers: Option<ConfigRouteHeader>,

    /// Optional: will route to hostname IF path *ends* with the given suffix.
    pub path_suffix: Option<String>,

    /// Optional: will route to hostname IF path *starts* with the given prefix.
    pub path_prefix: Option<String>,

    /// The upstreams to which the request will be proxied,
    pub upstreams: Vec<ConfigRouteUpstream>,
}

#[derive(Debug, Serialize, Deserialize, PartialEq)]
pub enum LogLevel {
    DEBUG,
    INFO,
    WARN,
    ERROR,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ConfigLogging {
    /// The level of logging to be used.
    #[serde(deserialize_with = "log_level_deser")]
    pub level: LogLevel,

    /// Whether to log access logs (request, duration, headers etc)
    pub access_logs: bool,

    /// Whether to log error logs (errors, panics, etc) from the rust runtime
    pub error_logs: bool,
}

/// The main configuration struct.
/// A configuration file (YAML, TOML or through ENV) will be parsed into this struct.
/// Example:
///
/// ```yaml
///
/// # Example configuration file
/// service_name: "proksi"
/// logging:
///   level: "INFO"
///   access_logs: true
///   error_logs: false
/// paths:
///   config_file: "/etc/proksi/config.toml"
///   tls_certificates: "/etc/proksi/certificates"
///   tls_challenges: "/etc/proksi/challenges"
///   tls_order: "/etc/proksi/orders"
///   tls_account_credentials: "/etc/proksi/account"
/// routes:
///   - host: "example.com"
///     path_prefix: "/api"
///     headers:
///       add:
///         - name: "X-Forwarded-For"
///           value: "<value>"
///         - name: "X-Api-Version"
///           value: "1.0"
///       remove:
///         - name: "Server"
///     upstreams:
///       - ip: "10.1.2.24/24"
///         port: 3000
///         network: "public"
///       - ip: "10.1.2.23/24"
///         port: 3000
///         network: "shared"
/// ```
///
#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct Config {
    /// The name of the service (will appear as a log property)
    #[serde(default)]
    pub service_name: String,

    /// General config
    pub logging: Option<ConfigLogging>,

    /// Configuration for paths (TLS, config file, etc.)
    pub paths: Option<ConfigPath>,

    /// The routes to be proxied to.
    pub routes: Vec<ConfigRoute>,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            service_name: "proksi".to_string(),
            routes: vec![],
            logging: Some(ConfigLogging {
                level: LogLevel::INFO,
                access_logs: true,
                error_logs: false,
            }),
            paths: Some(ConfigPath {
                tls_certificates: "/etc/proksi/tls/certificates".to_string(),
                tls_challenges: "/etc/proksi/tls/challenges".to_string(),
                tls_order: "/etc/proksi/tls/orders".to_string(),
                tls_account_credentials: "/etc/proksi/tls/account".to_string(),
            }),
        }
    }
}

impl Config {
    // Allow the configuration to be extracted from any `Provider`.
    fn from<T: figment::Provider>(provider: T) -> Result<Config, figment::Error> {
        Figment::from(provider).extract()
    }

    // Provide a default provider, a `Figment`.
    fn figment() -> Figment {
        use figment::providers::Env;

        // In reality, whatever the library desires.
        Figment::from(Config::default()).merge(Env::prefixed("APP_"))
    }
}

/// Implement the `Provider` trait for the `Config` struct.
/// This allows the `Config` struct to be used as a configuration provider with *defaults*.
impl Provider for Config {
    fn metadata(&self) -> figment::Metadata {
        figment::Metadata::named("proksi")
    }

    fn data(
        &self,
    ) -> Result<figment::value::Map<figment::Profile, figment::value::Dict>, figment::Error> {
        figment::providers::Serialized::defaults(Config::default()).data()
    }
}

/// Load the configuration from the configuration file(s) as a `Config` struct.
/// In theory one could create all 3 configurations formats and they will overlap each other
///
/// Nested keys can be separated by double underscores (__) in the environment variables.
/// E.g. `PROKSI__LOGGING__LEVEL=DEBUG` will set the `level` key in the `logging` key in the `proksi` key.
pub fn load_proxy_config(config_path: &str) -> Result<Config, figment::Error> {
    let config: Config = Figment::new()
        .merge(Config::default())
        .merge(Yaml::file(format!("{}/proksi-config.yaml", config_path)))
        .merge(Toml::file(format!("{}/proksi-config.toml", config_path)))
        .merge(Env::prefixed("PROKSI_").split("__"))
        .extract()?;

    Ok(config)
}

fn log_level_deser<'de, D>(deserializer: D) -> Result<LogLevel, D::Error>
where
    D: Deserializer<'de>,
{
    let s = String::deserialize(deserializer)?;
    match s.to_uppercase().as_str() {
        "DEBUG" => Ok(LogLevel::DEBUG),
        "INFO" => Ok(LogLevel::INFO),
        "WARN" => Ok(LogLevel::WARN),
        "ERROR" => Ok(LogLevel::ERROR),
        _ => Err(serde::de::Error::custom(
            "expected one of DEBUG, INFO, WARN, ERROR",
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn helper_config_file() -> &'static str {
        r#"
        service_name: "proksi"
        logging:
          level: "INFO"
          access_logs: true
          error_logs: false

        routes:
          - host: "example.com"
            path_prefix: "/api"
            headers:
              add:
                - name: "X-Forwarded-For"
                  value: "<value>"
                - name: "X-Api-Version"
                  value: "1.0"
              remove:
                - name: "Server"
            upstreams:
              - ip: "10.0.1.3/25"
                port: 3000
                network: "public"
      "#
    }

    #[test]
    fn test_load_config_from_yaml() {
        figment::Jail::expect_with(|jail| {
            let tmp_dir = jail.directory().to_string_lossy();

            jail.create_file(
                format!("{}/proksi-config.yaml", tmp_dir),
                helper_config_file(),
            )?;

            let config = load_proxy_config(&tmp_dir);
            let proxy_config = config.unwrap();
            assert_eq!(proxy_config.service_name, "proksi");

            Ok(())
        });
    }

    #[test]
    fn test_load_config_from_yaml_and_env_vars() {
        figment::Jail::expect_with(|jail| {
            jail.create_file(
                format!("{}/proksi-config.yaml", jail.directory().to_str().unwrap()),
                helper_config_file(),
            )?;
            jail.set_env("PROKSI_SERVICE_NAME", "new_name");
            jail.set_env("PROKSI_LOGGING__LEVEL", "warn");
            jail.set_env(
                "PROKSI_ROUTES",
                r#"[{
              host="changed.example.com",
              upstreams=[{ ip="10.0.1.2/24", port=3000, weight=1 }] }]
            "#,
            );

            let config = load_proxy_config(jail.directory().to_str().unwrap());

            let proxy_config = config.unwrap();
            assert_eq!(proxy_config.service_name, "new_name");
            assert_eq!(proxy_config.logging.unwrap().level, LogLevel::WARN);
            assert_eq!(proxy_config.routes[0].host, "changed.example.com");
            assert_eq!(proxy_config.routes[0].upstreams[0].ip, "10.0.1.2/24");

            Ok(())
        });
    }

    #[test]
    fn test_load_config_with_defaults_only() {
        let config = load_proxy_config("/tmp");
        let proxy_config = config.unwrap();
        let logging = proxy_config.logging.unwrap();
        assert_eq!(proxy_config.service_name, "proksi");
        assert_eq!(logging.level, LogLevel::INFO);
        assert_eq!(logging.access_logs, true);
        assert_eq!(logging.error_logs, false);
        assert_eq!(proxy_config.routes.len(), 0);
    }

    #[test]
    fn test_load_config_with_defaults_and_yaml() {
        figment::Jail::expect_with(|jail| {
            let tmp_dir = jail.directory().to_string_lossy();

            jail.create_file(
                format!("{}/proksi-config.yaml", tmp_dir),
                r#"
                routes:
                  - host: "example.com"
                    upstreams:
                      - ip: "10.1.2.24/24"
                        port: 3000
                "#,
            )?;

            let config = load_proxy_config(&tmp_dir);
            let proxy_config = config.unwrap();
            let logging = proxy_config.logging.unwrap();
            let paths = proxy_config.paths.unwrap();

            assert_eq!(proxy_config.service_name, "proksi");
            assert_eq!(logging.level, LogLevel::INFO);
            assert_eq!(logging.access_logs, true);
            assert_eq!(logging.error_logs, false);
            assert_eq!(proxy_config.routes.len(), 1);

            assert_eq!(paths.tls_account_credentials, "/etc/proksi/tls/account");
            assert_eq!(paths.tls_certificates, "/etc/proksi/tls/certificates");

            Ok(())
        });
    }
}
