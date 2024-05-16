use clap::{Args, Parser, ValueEnum};
use figment::{
    providers::{Env, Format, Serialized, Toml, Yaml},
    Figment, Provider,
};
use serde::{Deserialize, Deserializer, Serialize};
use tracing::level_filters::LevelFilter;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ConfigLetsEncrypt {
    /// The email to use for the let's encrypt account
    pub email: String,
    /// Whether to enable the background service that renews the certificates (default: true)
    pub enabled: Option<bool>,

    /// Use the staging let's encrypt server (default: true)
    pub staging: Option<bool>,
}

impl Default for ConfigLetsEncrypt {
    fn default() -> Self {
        Self {
            email: "contact@example.com".to_string(),
            enabled: Some(true),
            staging: Some(true),
        }
    }
}

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

impl Default for ConfigPath {
    fn default() -> Self {
        Self {
            tls_certificates: "/etc/proksi/tls/certificates".to_string(),
            tls_challenges: "/etc/proksi/tls/challenges".to_string(),
            tls_order: "/etc/proksi/tls/orders".to_string(),
            tls_account_credentials: "/etc/proksi/tls/account".to_string(),
        }
    }
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

#[derive(Debug, Serialize, Deserialize, PartialEq, Clone, Copy, ValueEnum)]
pub enum LogLevel {
    Debug,
    Info,
    Warn,
    Error,
}

/// Transforms our custom LogLevel enum into a `tracing::level_filters::LevelFilter`
/// enum used by the `tracing` crate.
impl From<&LogLevel> for tracing::level_filters::LevelFilter {
    fn from(val: &LogLevel) -> Self {
        match val {
            LogLevel::Debug => LevelFilter::DEBUG,
            LogLevel::Info => LevelFilter::INFO,
            LogLevel::Warn => LevelFilter::WARN,
            LogLevel::Error => LevelFilter::ERROR,
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, Args)]
#[group(id = "logging", requires = "level")]
pub struct ConfigLogging {
    /// The level of logging to be used.
    #[serde(deserialize_with = "log_level_deser")]
    #[arg(long, required = false, value_enum, default_value = "info")]
    pub level: LogLevel,

    /// Whether to log access logs (request, duration, headers etc).
    #[arg(long, required = false, value_parser, default_value = "true")]
    pub access_logs_enabled: bool,

    /// Whether to log error logs (errors, panics, etc) from the Rust runtime.
    #[arg(long, required = false, value_parser, default_value = "false")]
    pub error_logs_enabled: bool,
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
///   access_logs_enabled: true
///   error_logs_enabled: false
/// letsencrypt:
///   enabled: true
///   email: "youremail@example.com"
///   production: true
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
#[derive(Debug, Serialize, Deserialize, Parser)]
#[command(name = "Proksi")]
#[command(version, about, long_about = None)]
pub(crate) struct Config {
    /// The name of the service (will appear as a log property)
    #[serde(default)]
    #[clap(short, long, default_value = "proksi")]
    pub service_name: String,

    /// The number of worker threads to be used by the HTTPS proxy service.
    ///
    /// For background services the default is always (1) and cannot be changed.
    #[clap(short, long, default_value = "1")]
    pub worker_threads: Option<usize>,

    /// The PATH to the configuration file to be used.
    ///
    /// The configuration file should be named either `proksi.toml`, `proksi.yaml` or `proksi.yml`
    ///
    /// and be present in that path. Defaults to the current directory.
    #[serde(skip)]
    #[clap(short, long, default_value = "./")]
    pub config_path: String,

    /// General config
    #[command(flatten)]
    pub logging: ConfigLogging,

    #[clap(skip)]
    pub letsencrypt: ConfigLetsEncrypt,

    /// Configuration for paths (TLS, config file, etc.)
    #[clap(skip)]
    pub paths: ConfigPath,

    /// The routes to be proxied to.
    #[clap(skip)]
    pub routes: Vec<ConfigRoute>,
    // Listeners -- a list of specific listeners and upstrems
    // that don't necessarily need to be HTTP/HTTPS related
    // pub listeners: Vec<ConfigListener>,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            config_path: "/etc/proksi/config".to_string(),
            service_name: "proksi".to_string(),
            worker_threads: Some(1),
            letsencrypt: ConfigLetsEncrypt::default(),
            routes: vec![],
            logging: ConfigLogging {
                level: LogLevel::Info,
                access_logs_enabled: true,
                error_logs_enabled: false,
            },
            paths: ConfigPath::default(),
        }
    }
}

// impl Config {
//     // Allow the configuration to be extracted from any `Provider`.
//     fn from<T: figment::Provider>(provider: T) -> Result<Config, figment::Error> {
//         Figment::from(provider).extract()
//     }

//     // Provide a default provider, a `Figment`.
//     fn figment() -> Figment {
//         use figment::providers::Env;

//         // In reality, whatever the library desires.
//         Figment::from(Config::default()).merge(Env::prefixed("APP_"))
//     }
// }

/// Implement the `Provider` trait for the `Config` struct.
/// This allows the `Config` struct to be used as a configuration provider with *defaults*.
impl Provider for Config {
    fn metadata(&self) -> figment::Metadata {
        figment::Metadata::named("proksi")
    }

    fn data(
        &self,
    ) -> Result<figment::value::Map<figment::Profile, figment::value::Dict>, figment::Error> {
        Serialized::defaults(Config::default()).data()
    }
}

/// Load the configuration from the configuration file(s) as a `Config` struct.
/// In theory one could create all 3 configurations formats and they will overlap each other
///
/// Nested keys can be separated by double underscores (__) in the environment variables.
/// E.g. `PROKSI__LOGGING__LEVEL=DEBUG` will set the `level` key in the `logging` key in the `proksi` key.
pub fn load_proxy_config(fallback: &str) -> Result<Config, figment::Error> {
    let parsed_commands = Config::parse();

    let path_with_fallback = if parsed_commands.config_path.is_empty() {
        fallback
    } else {
        parsed_commands.config_path.as_str()
    };

    let config: Config = Figment::new()
        .merge(Config::default())
        .merge(Serialized::defaults(&parsed_commands))
        .merge(Yaml::file(format!("{}/proksi.yaml", path_with_fallback)))
        .merge(Toml::file(format!("{}/proksi.toml", path_with_fallback)))
        .merge(Env::prefixed("PROKSI_").split("__"))
        .extract()?;

    Ok(config)
}

fn log_level_deser<'de, D>(deserializer: D) -> Result<LogLevel, D::Error>
where
    D: Deserializer<'de>,
{
    let s = String::deserialize(deserializer)?;
    match s.to_lowercase().as_str() {
        "debug" => Ok(LogLevel::Debug),
        "info" => Ok(LogLevel::Info),
        "warn" => Ok(LogLevel::Warn),
        "error" => Ok(LogLevel::Error),
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
          access_logs_enabled: true
          error_logs_enabled: false

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

            jail.create_file(format!("{}/proksi.yaml", tmp_dir), helper_config_file())?;

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
                format!("{}/proksi.yaml", jail.directory().to_str().unwrap()),
                helper_config_file(),
            )?;
            jail.set_env("PROKSI_SERVICE_NAME", "new_name");
            jail.set_env("PROKSI_LOGGING__LEVEL", "warn");
            jail.set_env("PROKSI_LETSENCRYPT__STAGING", "false");
            jail.set_env("PROKSI_LETSENCRYPT__EMAIL", "my-real-email@domain.com");
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
            assert_eq!(proxy_config.logging.level, LogLevel::Warn);

            assert_eq!(proxy_config.letsencrypt.staging, Some(false));
            assert_eq!(proxy_config.letsencrypt.email, "my-real-email@domain.com");

            assert_eq!(proxy_config.routes[0].host, "changed.example.com");
            assert_eq!(proxy_config.routes[0].upstreams[0].ip, "10.0.1.2/24");

            Ok(())
        });
    }

    #[test]
    fn test_load_config_with_defaults_only() {
        figment::Jail::expect_with(|_| {
            let config = load_proxy_config("/non-existent");
            let proxy_config = config.unwrap();

            let logging = proxy_config.logging;
            assert_eq!(proxy_config.service_name, "proksi");
            assert_eq!(logging.level, LogLevel::Info);
            assert_eq!(logging.access_logs_enabled, true);
            assert_eq!(logging.error_logs_enabled, false);

            print!("{:?}", proxy_config.routes);

            assert_eq!(proxy_config.routes.len(), 0);

            Ok(())
        })
    }

    #[test]
    fn test_load_config_with_defaults_and_yaml() {
        figment::Jail::expect_with(|jail| {
            let tmp_dir = jail.directory().to_string_lossy();

            jail.create_file(
                format!("{}/proksi.yaml", tmp_dir),
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
            let logging = proxy_config.logging;
            let paths = proxy_config.paths;
            let letsencrypt = proxy_config.letsencrypt;

            assert_eq!(proxy_config.service_name, "proksi");
            assert_eq!(logging.level, LogLevel::Info);
            assert_eq!(logging.access_logs_enabled, true);
            assert_eq!(logging.error_logs_enabled, false);
            assert_eq!(proxy_config.routes.len(), 1);

            assert_eq!(letsencrypt.email, "contact@example.com");
            assert_eq!(letsencrypt.enabled, Some(true));
            assert_eq!(letsencrypt.staging, Some(true));

            assert_eq!(paths.tls_account_credentials, "/etc/proksi/tls/account");
            assert_eq!(paths.tls_certificates, "/etc/proksi/tls/certificates");

            Ok(())
        });
    }
}
