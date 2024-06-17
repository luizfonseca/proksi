use std::{borrow::Cow, collections::HashMap, path::PathBuf};

use clap::{Args, Parser, ValueEnum};
use figment::{
    providers::{Env, Format, Serialized, Toml, Yaml},
    Figment, Provider,
};
use serde::{Deserialize, Deserializer, Serialize};
use tracing::level_filters::LevelFilter;

mod validate;

/// Default fn for boolean values
fn bool_true() -> bool {
    true
}

fn default_proto_version() -> ProtoVersion {
    ProtoVersion::V1_3
}

fn default_proto_version_min() -> ProtoVersion {
    ProtoVersion::V1_2
}

#[derive(Debug, Serialize, Deserialize, Clone, ValueEnum)]
pub(crate) enum DockerServiceMode {
    Swarm,
    Container,
}

#[derive(Debug, Serialize, Deserialize, Clone, Args)]
#[group(id = "docker", requires = "level")]
pub struct Docker {
    /// The interval (in seconds) to check for label updates
    /// (default: every 15 seconds)
    #[arg(
        long = "docker.interval_secs",
        required = false,
        value_parser,
        default_value = "15"
    )]
    pub interval_secs: Option<u64>,

    /// The docker endpoint to connect to (can be a unix socket or a tcp address)
    #[arg(
        long = "docker.endpoint",
        required = false,
        value_parser,
        default_value = "unix:///var/run/docker.sock"
    )]
    pub endpoint: Option<Cow<'static, str>>,

    /// Enables the docker label service
    /// (default: false)
    #[arg(
        long = "docker.enabled",
        required = false,
        value_parser,
        default_value = "false",
        id = "docker.enabled"
    )]
    pub enabled: Option<bool>,

    /// Mode to use for the docker service
    #[serde(deserialize_with = "docker_mode_deser")]
    #[arg(
        long = "docker.mode",
        required = false,
        value_enum,
        default_value = "container"
    )]
    pub mode: DockerServiceMode,
}

impl Default for Docker {
    fn default() -> Self {
        Self {
            interval_secs: Some(15),
            endpoint: Some(Cow::Borrowed("unix:///var/run/docker.sock")),
            enabled: Some(false),
            mode: DockerServiceMode::Container,
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct LetsEncrypt {
    /// The email to use for the let's encrypt account
    pub email: Cow<'static, str>,
    /// Whether to enable the background service that renews the certificates (default: true)
    pub enabled: Option<bool>,

    /// Use the staging let's encrypt server (default: true)
    pub staging: Option<bool>,
}

impl Default for LetsEncrypt {
    fn default() -> Self {
        Self {
            email: Cow::Borrowed("contact@example.com"),
            enabled: Some(true),
            staging: Some(true),
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Path {
    // TLS
    /// Path to the certificates directory (where the certificates are stored)
    pub lets_encrypt: PathBuf,
}

impl Default for Path {
    fn default() -> Self {
        Self {
            lets_encrypt: PathBuf::from("/etc/proksi/letsencrypt"),
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct RouteHeaderAdd {
    /// The name of the header
    pub name: Cow<'static, str>,

    /// The value of the header
    pub value: Cow<'static, str>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct RouteHeaderRemove {
    /// The name of the header to remove (ex.: "Server")
    pub name: Cow<'static, str>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct RouteHeader {
    /// The name of the header
    pub add: Option<Vec<RouteHeaderAdd>>,

    /// The value of the header
    pub remove: Option<Vec<RouteHeaderRemove>>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct RouteUpstream {
    /// The TCP address of the upstream (ex. 10.0.0.1/24 etc)
    pub ip: Cow<'static, str>,

    /// The port of the upstream (ex: 3000, 5000, etc.)
    pub port: u16,

    /// The network of the upstream (ex: 'public', 'shared') -- useful for docker discovery
    pub network: Option<String>,

    /// Optional: The weight of the upstream (ex: 1, 2, 3, etc.) --
    /// used for weight-based load balancing.
    pub weight: Option<i8>,

    pub sni: Option<String>,

    pub headers: Option<RouteHeader>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct RouteSslCertificate {
    /// Whether to use a self-signed certificate if the certificate can't be
    /// retrieved from the path or object storage (or generated from letsencrypt)
    /// (defaults to true)
    pub self_signed_on_failure: Option<bool>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct RoutePathMatcher {
    /// Optional: pattern to match the path
    /// (ex: /api/v1/*)
    pub patterns: Vec<Cow<'static, str>>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct RouteMatcher {
    pub path: Option<RoutePathMatcher>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct RoutePlugin {
    /// The name of the plugin (must be a valid plugin name)
    pub name: Cow<'static, str>,

    /// The configuration for the plugin - we are not enforcing a specific format.
    /// Each plugin is in charge of parsing the configuration.
    /// The configuration is a key-value pair where the key is a string and
    /// the value is a JSON object (ex: `{ "key": "value" }`)
    pub config: Option<HashMap<Cow<'static, str>, serde_json::Value>>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct RouteSslPath {
    /// Path to the certificate .key file (e.g. `/etc/proksi/certs/my-host.key`)
    pub key: PathBuf,

    /// Path to the certificate .pem file (e.g. `/etc/proksi/certs/my-host.pem`)
    pub pem: PathBuf,
}

#[derive(Debug, Serialize, Deserialize)]
pub enum ProtoVersion {
    V1_1,
    V1_2,
    V1_3,
}

/// Converts a `pingora::tls::ssl::SslVersion` to a `ProtoVersion`
impl From<pingora::tls::ssl::SslVersion> for ProtoVersion {
    fn from(v: pingora::tls::ssl::SslVersion) -> Self {
        match v {
            pingora::tls::ssl::SslVersion::TLS1_1 => ProtoVersion::V1_1,
            pingora::tls::ssl::SslVersion::TLS1_2 => ProtoVersion::V1_2,
            _ => ProtoVersion::V1_3,
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct RouteSsl {
    /// If provided, will be used instead of generating certificates from
    /// Let's Encrypt or self-signed certificates.
    pub path: Option<RouteSslPath>,

    /// The minimum and maximum protocol versions that the client can use.
    #[serde(
        default = "default_proto_version_min",
        deserialize_with = "proto_version_deser"
    )]
    pub min_proto: ProtoVersion,

    /// The maximum protocol version that the client can use.
    #[serde(
        default = "default_proto_version",
        deserialize_with = "proto_version_deser"
    )]
    pub max_proto: ProtoVersion,

    /// If the `self_signed_on_failure` is set to <true>,
    /// the server will use a self-signed certificate if the Let's Encrypt certificate
    /// issuance fails. This is useful for development and testing purposes.
    ///
    /// If the `self_signed_on_failure` is set to <false>
    /// and let's encrypt fails to issue a certificate,
    /// the server will respond with a SNI error that closes the connection.
    ///
    /// The default value is <true>.
    #[serde(default = "bool_true")]
    pub self_signed_fallback: bool,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Route {
    /// The hostname that the proxy will accept
    /// requests for the upstreams in the route.
    /// (ex: 'example.com', 'www.example.com', etc.)
    ///
    /// This is the host header that the proxy will match and will
    /// also be used to create the certificate for the domain when `letsencrypt` is enabled.
    pub host: Cow<'static, str>,

    /// Plugins that will be applied to the route/host
    /// (ex: rate limiting, oauth2, etc.)
    pub plugins: Option<Vec<RoutePlugin>>,

    /// SSL certificate configurations for the given host
    /// (ex: self-signed, path/object storage, etc.)
    pub ssl_certificate: Option<RouteSslCertificate>,

    /// SSL configuration for the route
    pub ssl: Option<RouteSsl>,

    /// Header modifications for the given route (remove, add, etc. )
    pub headers: Option<RouteHeader>,

    /// The upstreams to which the request will be proxied,
    pub upstreams: Vec<RouteUpstream>,

    /// The matcher for the route
    /// (ex: path, query, etc.)
    pub match_with: Option<RouteMatcher>,
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Clone, ValueEnum)]
pub enum LogLevel {
    Debug,
    Info,
    Warn,
    Error,
    Trace,
}

/// Transforms our custom `LogLevel` enum into a `tracing::level_filters::LevelFilter`
/// enum used by the `tracing` crate.
impl From<&LogLevel> for tracing::level_filters::LevelFilter {
    fn from(val: &LogLevel) -> Self {
        match val {
            LogLevel::Debug => LevelFilter::DEBUG,
            LogLevel::Info => LevelFilter::INFO,
            LogLevel::Warn => LevelFilter::WARN,
            LogLevel::Error => LevelFilter::ERROR,
            LogLevel::Trace => LevelFilter::TRACE,
        }
    }
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Clone, ValueEnum)]
pub enum LogFormat {
    Json,
    Pretty,
}

#[derive(Debug, Serialize, Deserialize, Clone, Args)]
#[group(id = "logging", requires = "level")]
pub struct Logging {
    /// If logging is enabled at all. Setting this to `false` will disable all logging output.
    #[arg(
        long = "log.enabled",
        required = false,
        value_parser,
        default_value = "true",
        id = "log.enabled"
    )]
    pub enabled: bool,

    /// The level of logging to be used.
    #[serde(deserialize_with = "log_level_deser")]
    #[arg(
        long = "log.level",
        required = false,
        value_enum,
        default_value = "info"
    )]
    pub level: LogLevel,

    /// Whether to log access logs (request, duration, headers etc).
    #[arg(
        long = "log.access_logs_enabled",
        required = false,
        value_parser,
        default_value = "true"
    )]
    pub access_logs_enabled: bool,

    /// Whether to log error logs (errors, panics, etc) from the Rust runtime.
    #[arg(
        long = "log.error_logs_enabled",
        required = false,
        value_parser,
        default_value = "true"
    )]
    pub error_logs_enabled: bool,

    /// The format of the log output
    #[serde(deserialize_with = "log_format_deser")]
    #[arg(
        long = "log.format",
        required = false,
        value_enum,
        default_value = "json"
    )]
    pub format: LogFormat,
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
///   lets_encrypt: "/etc/proksi/letsencrypt"
/// routes:
///   - host: "example.com"
///     match_with:
///       path:
///         patterns:
///          - "/api/v1/*"
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
    pub service_name: Cow<'static, str>,

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
    #[allow(clippy::struct_field_names)]
    pub config_path: Cow<'static, str>,

    /// General config
    #[command(flatten)]
    pub logging: Logging,

    #[command(flatten)]
    pub docker: Docker,

    #[clap(skip)]
    pub lets_encrypt: LetsEncrypt,

    /// Configuration for paths (TLS, config file, etc.)
    #[clap(skip)]
    pub paths: Path,

    /// The routes to be proxied to.
    #[clap(skip)]
    pub routes: Vec<Route>,
    // Listeners -- a list of specific listeners and upstrems
    // that don't necessarily need to be HTTP/HTTPS related
    // pub listeners: Vec<ConfigListener>,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            config_path: Cow::Borrowed("/etc/proksi/config"),
            service_name: Cow::Borrowed("proksi"),
            worker_threads: Some(1),
            docker: Docker::default(),
            lets_encrypt: LetsEncrypt::default(),
            routes: vec![],
            logging: Logging {
                enabled: true,
                level: LogLevel::Info,
                access_logs_enabled: true,
                error_logs_enabled: false,
                format: LogFormat::Json,
            },
            paths: Path::default(),
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
/// E.g. `PROKSI__LOGGING__LEVEL=DEBUG` will set the `level` key in the
/// `logging` key in the `proksi` key.
pub fn load(fallback: &str) -> Result<Config, figment::Error> {
    let parsed_commands = Config::parse();

    let path_with_fallback = if parsed_commands.config_path.is_empty() {
        fallback
    } else {
        &parsed_commands.config_path
    };

    let config: Config = Figment::new()
        .merge(Config::default())
        .merge(Serialized::defaults(&parsed_commands))
        .merge(Yaml::file(format!("{path_with_fallback}/proksi.yml")))
        .merge(Yaml::file(format!("{path_with_fallback}/proksi.yaml")))
        .merge(Toml::file(format!("{path_with_fallback}/proksi.toml")))
        .merge(Env::prefixed("PROKSI_").split("__"))
        .extract()?;

    // validate configuration and throw error upwards
    validate::check_config(&config).map_err(|err| figment::Error::from(err.to_string()))?;

    Ok(config)
}

/// Deserialize function to convert a string to a `LogLevel` Enum
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
        "trace" => Ok(LogLevel::Trace),
        _ => Err(serde::de::Error::custom(
            "expected one of DEBUG, INFO, WARN, ERROR, TRACE",
        )),
    }
}

/// Deserialize function to convert a string to a `DockerServiceMode` Enum
fn docker_mode_deser<'de, D>(deserializer: D) -> Result<DockerServiceMode, D::Error>
where
    D: Deserializer<'de>,
{
    let s = String::deserialize(deserializer)?;
    match s.to_lowercase().as_str() {
        "swarm" => Ok(DockerServiceMode::Swarm),
        "container" => Ok(DockerServiceMode::Container),
        _ => Err(serde::de::Error::custom(
            "expected one of: Swarm, Container",
        )),
    }
}

/// Deserialize function to convert a string to a `LogLevel` Enum
fn log_format_deser<'de, D>(deserializer: D) -> Result<LogFormat, D::Error>
where
    D: Deserializer<'de>,
{
    let s = String::deserialize(deserializer)?;
    match s.to_lowercase().as_str() {
        "json" => Ok(LogFormat::Json),
        "pretty" => Ok(LogFormat::Pretty),
        _ => Err(serde::de::Error::custom("expected one of: json, pretty")),
    }
}

/// Deserialize function to convert a string to a `LogLevel` Enum
fn proto_version_deser<'de, D>(deserializer: D) -> Result<ProtoVersion, D::Error>
where
    D: Deserializer<'de>,
{
    let s = String::deserialize(deserializer)?;
    match s.to_lowercase().as_str() {
        "v1.1" => Ok(ProtoVersion::V1_1),
        "v1.2" => Ok(ProtoVersion::V1_2),
        "v1.3" => Ok(ProtoVersion::V1_3),
        _ => Err(serde::de::Error::custom(
            "expected one of: v1.1, v1.2, v1.3",
        )),
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn helper_config_file() -> &'static str {
        r#"
        service_name: "proksi"
        lets_encrypt:
          email: "user@domain.net"
        logging:
          level: "INFO"
          access_logs_enabled: true
          error_logs_enabled: false
        paths:
          lets_encrypt: "/test/letsencrypt"
        routes:
          - host: "example.com"
            plugins:
              - name: "cors"
                config:
                  allowed_origins: ["*"]
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

            let config = load(&tmp_dir);
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
            jail.set_env("PROKSI_LOGGING__ENABLED", "false");
            jail.set_env("PROKSI_LOGGING__LEVEL", "warn");
            jail.set_env("PROKSI_DOCKER__ENABLED", "true");
            jail.set_env("PROKSI_DOCKER__INTERVAL_SECS", "30");
            jail.set_env("PROKSI_DOCKER__ENDPOINT", "http://localhost:2375");
            jail.set_env("PROKSI_LETS_ENCRYPT__STAGING", "false");
            jail.set_env("PROKSI_LETS_ENCRYPT__EMAIL", "my-real-email@domain.com");
            jail.set_env(
                "PROKSI_ROUTES",
                r#"[{
              host="changed.example.com",
              match_with={ path={ patterns=["/api/v1/:entity/:action*"] } },
              plugins=[{ name="cors", config={ allowed_origins=["*"] } }],
              upstreams=[{ ip="10.0.1.2/24", port=3000, weight=1 }] }]
            "#,
            );

            let config = load(jail.directory().to_str().unwrap());

            let proxy_config = config.unwrap();
            assert_eq!(proxy_config.service_name, "new_name");
            assert!(!proxy_config.logging.enabled);
            assert_eq!(proxy_config.logging.level, LogLevel::Warn);

            assert_eq!(proxy_config.docker.enabled, Some(true));
            assert_eq!(proxy_config.docker.interval_secs, Some(30));
            assert_eq!(
                proxy_config.docker.endpoint,
                Some(Cow::Borrowed("http://localhost:2375"))
            );

            assert_eq!(proxy_config.lets_encrypt.staging, Some(false));
            assert_eq!(proxy_config.lets_encrypt.email, "my-real-email@domain.com");

            assert_eq!(proxy_config.routes[0].host, "changed.example.com");
            assert_eq!(proxy_config.routes[0].upstreams[0].ip, "10.0.1.2/24");

            let matcher = proxy_config.routes[0].match_with.as_ref().unwrap();

            assert_eq!(
                matcher.path.as_ref().unwrap().patterns,
                vec![Cow::Borrowed("/api/v1/:entity/:action*")]
            );

            assert_eq!(
                proxy_config.paths.lets_encrypt,
                PathBuf::from("/test/letsencrypt")
            );
            Ok(())
        });
    }

    #[test]
    fn test_load_config_with_defaults_only() {
        figment::Jail::expect_with(|jail| {
            jail.set_env("PROKSI_LETS_ENCRYPT__EMAIL", "my-real-email@domain.com");
            let config = load("/non-existent");
            let proxy_config = config.unwrap();

            let logging = proxy_config.logging;
            assert_eq!(proxy_config.service_name, "proksi");
            assert_eq!(logging.level, LogLevel::Info);
            assert!(logging.access_logs_enabled);
            assert!(logging.error_logs_enabled);

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
                lets_encrypt:
                  email: "domain@valid.com"
                routes:
                  - host: "example.com"
                    upstreams:
                      - ip: "10.1.2.24/24"
                        port: 3000
                    plugins:
                      - name: "cors"
                        config:
                          allowed_origins: ["*"]
                    ssl:
                      path:
                        key: "/etc/proksi/certs/my-host.key"
                        pem: "/etc/proksi/certs/my-host.pem"
                "#,
            )?;

            let config = load(&tmp_dir);
            let proxy_config = config.unwrap();
            let logging = proxy_config.logging;
            let paths = proxy_config.paths;
            let letsencrypt = proxy_config.lets_encrypt;

            assert_eq!(proxy_config.service_name, "proksi");
            assert_eq!(logging.level, LogLevel::Info);
            assert!(logging.access_logs_enabled);
            assert!(logging.error_logs_enabled);
            assert_eq!(proxy_config.routes.len(), 1);

            assert_eq!(proxy_config.docker.enabled, Some(false));
            assert_eq!(proxy_config.docker.interval_secs, Some(15));
            assert_eq!(
                proxy_config.docker.endpoint,
                Some(Cow::Borrowed("unix:///var/run/docker.sock"))
            );

            assert_eq!(letsencrypt.email, "domain@valid.com");
            assert_eq!(letsencrypt.enabled, Some(true));
            assert_eq!(letsencrypt.staging, Some(true));

            assert_eq!(paths.lets_encrypt.as_os_str(), "/etc/proksi/letsencrypt");

            let route = &proxy_config.routes[0];
            let plugins = route.plugins.as_ref().unwrap();
            let plugin_config = plugins[0].config.as_ref().unwrap();
            assert_eq!(plugins[0].name, "cors");
            assert_eq!(plugin_config.get("allowed_origins"), Some(&json!(["*"])));

            let ssl = route.ssl.as_ref().unwrap();
            let path = ssl.path.as_ref().unwrap();
            assert_eq!(ssl.self_signed_fallback, true);
            assert_eq!(path.key.as_os_str(), "/etc/proksi/certs/my-host.key");
            assert_eq!(path.pem.as_os_str(), "/etc/proksi/certs/my-host.pem");

            Ok(())
        });
    }
}
