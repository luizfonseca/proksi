use bytes::Bytes;
use clap::crate_version;
use config::{Config, load, LogFormat, RouteHeaderAdd, RouteHeaderRemove, RoutePlugin, migrate_server_config, is_legacy_config};
use stores::{MemoryStore, global::init_store};
use tracing_subscriber::EnvFilter;
use server::{ProxyServerManager, manager};

use std::{borrow::Cow, sync::Arc};

mod cache;
mod channel;
mod config;
mod plugins;
mod proxy_server;
mod proxy_service;
mod server;
mod services;
mod stores;
mod tools;
mod wasm;

#[derive(Clone, Default)]
pub struct MsgRoute {
    host: Cow<'static, str>,
    upstreams: Vec<String>,
    path_matchers: Vec<String>,
    host_headers_add: Vec<RouteHeaderAdd>,
    host_headers_remove: Vec<RouteHeaderRemove>,
    plugins: Vec<RoutePlugin>,

    self_signed_certs: bool,
}

#[derive(Clone)]
pub struct MsgCert {
    _cert: Bytes,
    _key: Bytes,
}

#[derive(Clone)]
pub enum MsgProxy {
    NewRoute(MsgRoute),
    NewCertificate(MsgCert),
    ConfigUpdate(()),
}

#[deny(
    clippy::all,
    clippy::pedantic,
    clippy::perf,
    clippy::correctness,
    clippy::style,
    clippy::suspicious,
    clippy::complexity
)]
fn main() -> Result<(), anyhow::Error> {
    // Configuration can be refreshed on file change
    // Loads configuration from command-line, YAML or TOML sources
    let proxy_config = Arc::new(load("/etc/proksi/configs").expect("Failed to load configuration"));

    // Validate configuration
    let proxies = if is_legacy_config(&proxy_config) {
        migrate_server_config(&proxy_config)
    } else {
        proxy_config.proxies.clone()
    };
    
    manager::validate_proxy_configs(&proxies)
        .expect("Invalid proxy configuration");

    // Initialize logging and get logger service
    let logger_service = setup_logging(proxy_config.clone());

    // Log configuration summary
    manager::log_proxy_configuration(&proxy_config);

    // Initialize global store based on configuration
    setup_store(&proxy_config);

    // Create and setup server manager
    let mut server_manager = ProxyServerManager::new(proxy_config.clone())?;
    server_manager.setup_services()?;
    
    // Add logger service to server
    server_manager.add_logger_service(logger_service);

    tracing::info!(
        version = crate_version!(),
        proxies = proxies.len(),
        "Proksi server starting with {} configured proxies",
        proxies.len(),
    );

    server_manager.run_forever();
}

fn setup_logging(proxy_config: Arc<Config>) -> services::logger::ProxyLoggerReceiver {
    // Logging channel
    let (log_sender, log_receiver) = tokio::sync::mpsc::unbounded_channel::<Vec<u8>>();

    let appender = services::logger::ProxyLog::new(
        log_sender,
        proxy_config.logging.enabled,
        proxy_config.logging.access_logs_enabled,
        proxy_config.logging.error_logs_enabled,
    );

    // Creates a tracing/logging subscriber based on the configuration provided
    if proxy_config.logging.format == LogFormat::Json {
        tracing_subscriber::fmt()
            .json()
            .with_env_filter(EnvFilter::from_default_env())
            .with_max_level(&proxy_config.logging.level)
            .with_writer(appender)
            .init();
    } else {
        tracing_subscriber::fmt()
            .with_env_filter(EnvFilter::from_default_env())
            .with_max_level(&proxy_config.logging.level)
            .with_ansi(proxy_config.logging.path.is_none())
            .with_writer(appender)
            .init();
    };

    services::logger::ProxyLoggerReceiver::new(log_receiver, &proxy_config)
}

fn setup_store(proxy_config: &Config) {
    match proxy_config.store.store_type {
        config::StoreType::Memory => {
            tracing::info!("using Memory store for certificates");
            init_store(MemoryStore::new());
        }
        config::StoreType::Redis => {
            let redis_url =
                proxy_config.store.redis_url.as_deref().expect(
                    "Failed to get redis_url from configuration when store type is 'redis'",
                );
            let redis_store = stores::RedisStore::new(redis_url)
                .expect("Failed to initialize Redis store");
            tracing::info!("using Redis store for certificates");
            init_store(redis_store);
        }
    };
}
