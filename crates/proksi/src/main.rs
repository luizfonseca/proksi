// Paths are formatted with `{:?}` on purpose, see `disallowed-methods` in clippy.toml
#![allow(clippy::unnecessary_debug_formatting)]

use ::pingora::server::Server;

use clap::crate_version;
use config::{load, LogFormat};
use stores::{global::init_store, MemoryStore};
use tracing_subscriber::EnvFilter;

use std::sync::{atomic::AtomicU64, Arc};

use pingora::server::configuration::Opt;

use services::{logger::ProxyLoggerReceiver, BackgroundFunctionService};

mod cache;
mod channel;
mod config;
mod plugins;
mod proxy_server;
mod routing;
mod server;
mod services;
mod stores;
mod tools;
mod wasm;

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
    let proxy_config = Arc::new(
        load("/etc/proksi/configs")
            .map_err(|err| anyhow::anyhow!("Failed to load configuration: {err}"))?,
    );

    // The addresses the hosts need (validated when loading the configuration)
    let listeners = config::listeners::listeners(&proxy_config).map_err(anyhow::Error::msg)?;
    let (default_https, default_http) =
        config::listeners::default_addresses(&proxy_config).map_err(anyhow::Error::msg)?;

    // Logging channel
    let (log_sender, log_receiver) = services::logger::log_queue();
    let dropped_logs = Arc::new(AtomicU64::new(0));

    // Route sources send their routes to the reconciler through this channel
    let (route_sender, route_receiver) = routing::service::snapshot_channel();
    let appender = services::logger::ProxyLog::new(
        log_sender,
        dropped_logs.clone(),
        proxy_config.logging.enabled,
        proxy_config.logging.access_logs_enabled,
        proxy_config.logging.error_logs_enabled,
    );

    init_tracing(&proxy_config, appender);
    for warning in config::deprecations::deprecation_warnings(&proxy_config) {
        tracing::warn!("{warning}");
    }

    // In-memory copy of the disk cache
    cache::disk::memory::init_memory_cache(proxy_config.cache.memory_limit_mb);

    // Initialize global store based on configuration
    match proxy_config.store.store_type {
        config::StoreType::Memory => {
            tracing::info!("using Memory store for certificates");
            init_store(MemoryStore::new());
        }
        config::StoreType::Redis => {
            let redis_url = proxy_config.store.redis_url.as_deref().ok_or_else(|| {
                anyhow::anyhow!(
                    "Failed to get redis_url from configuration when store type is 'redis'"
                )
            })?;
            let redis_store = stores::RedisStore::new(redis_url)
                .map_err(|err| anyhow::anyhow!("Failed to initialize Redis store: {err}"))?;
            tracing::info!("using Redis store for certificates");
            init_store(redis_store);
        }
        config::StoreType::Sqlite | config::StoreType::Postgres => {
            init_sql_store(&proxy_config.store)?;
        }
    }

    // Pingora load balancer server
    let pingora_opts = Opt {
        daemon: proxy_config.daemon,
        upgrade: proxy_config.upgrade,
        conf: None,
        nocapture: false,
        test: false,
    };

    let mut pingora_server = Server::new(Some(pingora_opts))?;
    pingora_server.bootstrap();

    // Add Prometheus service
    // let mut prometheus_service_http = Service::prometheus_http_service();
    // prometheus_service_http.add_tcp("0.0.0.0:9090");
    // pingora_server.add_service(prometheus_service_http);

    // Non-dedicated background services
    pingora_server.add_service(BackgroundFunctionService::new(
        proxy_config.clone(),
        route_sender,
        route_receiver,
    ));

    // Dedicated logger service
    pingora_server.add_service(ProxyLoggerReceiver::new(
        log_receiver,
        dropped_logs,
        &proxy_config,
    ));

    // Listen on the HTTP and HTTPS addresses the hosts need
    if !listeners.http.is_empty() {
        pingora_server.add_service(proxy_server::services::http_service(
            &pingora_server.configuration,
            &listeners.http,
            default_http,
            proxy_config.worker_threads,
        ));
    }
    if !listeners.https.is_empty() {
        pingora_server.add_service(proxy_server::services::https_service(
            &pingora_server.configuration,
            &listeners.https,
            default_https,
            proxy_config.worker_threads,
        )?);
    }
    let server_info = format!(
        "listening for HTTPS on {:?} and HTTP on {:?}",
        listeners.https, listeners.http
    );
    tracing::info!(
        version = crate_version!(),
        workers = proxy_config.worker_threads,
        server_info,
    );

    pingora_server.run_forever();
}

/// Creates the tracing/logging subscriber based on the configuration provided
fn init_tracing(proxy_config: &config::Config, appender: services::logger::ProxyLog) {
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
    }
}

/// Connects to the SQL store (sqlite or postgres), if proksi was built with support for it
fn init_sql_store(store: &config::StoreConfig) -> Result<(), anyhow::Error> {
    let supported = (store.store_type == config::StoreType::Sqlite
        && cfg!(feature = "store-sqlite"))
        || (store.store_type == config::StoreType::Postgres && cfg!(feature = "store-postgres"));
    if !supported {
        return Err(anyhow::anyhow!(
            "proksi was built without support for the {:?} store (cargo feature store-{})",
            store.store_type,
            format!("{:?}", store.store_type).to_lowercase()
        ));
    }

    #[cfg(any(feature = "store-sqlite", feature = "store-postgres"))]
    {
        let database_url = store.database_url.as_deref().ok_or_else(|| {
            anyhow::anyhow!(
                "store.database_url is required for the {:?} store",
                store.store_type
            )
        })?;
        let sql_store = stores::sql_store::SqlStore::connect(database_url)
            .map_err(|err| anyhow::anyhow!("Failed to initialize the SQL store: {err}"))?;
        tracing::info!("using {:?} store", store.store_type);
        init_store(sql_store);
    }

    Ok(())
}
