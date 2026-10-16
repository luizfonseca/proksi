// Paths are formatted with `{:?}` on purpose, see `disallowed-methods` in clippy.toml
#![allow(clippy::unnecessary_debug_formatting)]

use ::pingora::server::Server;

use clap::crate_version;
use config::{load, LogFormat};
use stores::{global::init_store, MemoryStore};
use tracing_subscriber::EnvFilter;

use std::sync::{atomic::AtomicU64, Arc};

use pingora::{listeners::tls::TlsSettings, proxy::http_proxy_service, server::configuration::Opt};

use proxy_server::cert_store::CertStore;
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

    let https_address = proxy_config
        .server
        .https_address
        .clone()
        .unwrap_or_default();
    let le_address = proxy_config.server.http_address.clone().unwrap_or_default();

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

    // Service: HTTP port (hosts without TLS, ACME challenges and redirects to HTTPS)
    let mut http_public_service = http_proxy_service(
        &pingora_server.configuration,
        proxy_server::https_proxy::Router {
            listener: proxy_server::listener::Listener::Http,
        },
    );

    http_public_service.add_tcp(&le_address);
    // Worker threads per configuration (the HTTP port also serves hosts without TLS)
    http_public_service.threads = proxy_config.worker_threads;

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

    // Listen on HTTP and HTTPS ports
    pingora_server.add_service(http_public_service);
    let server_info = if proxy_config.server.serves_https() {
        pingora_server.add_service(https_service(
            &pingora_server.configuration,
            &https_address,
            proxy_config.worker_threads,
        )?);
        format!("running HTTPS service on {https_address} and HTTP service on {le_address}")
    } else {
        format!("running HTTP service on {le_address} (HTTPS is disabled)")
    };
    tracing::info!(
        version = crate_version!(),
        workers = proxy_config.worker_threads,
        server_info,
    );

    pingora_server.run_forever();
}

/// The HTTPS port (main service): certificates by SNI, HTTP/2, and the router for hosts with TLS
fn https_service(
    configuration: &Arc<pingora::server::configuration::ServerConf>,
    https_address: &str,
    worker_threads: Option<usize>,
) -> Result<
    pingora::services::listening::Service<
        pingora::proxy::HttpProxy<proxy_server::https_proxy::Router>,
    >,
    anyhow::Error,
> {
    let router = proxy_server::https_proxy::Router {
        listener: proxy_server::listener::Listener::Https,
    };
    let mut service = http_proxy_service(configuration, router);
    service.threads = worker_threads;

    // Setup tls settings and Enable HTTP/2
    let mut tls_settings = TlsSettings::with_callbacks(Box::new(CertStore::new()))?;
    tls_settings.enable_h2();

    // tls_settings.set_session_cache_mode(SslSessionCacheMode::SERVER);
    tls_settings.set_servername_callback(move |ssl_ref, _| CertStore::sni_callback(ssl_ref));

    // For now this is a hardcoded recommendation based on
    // https://developers.cloudflare.com/ssl/reference/protocols/
    // but will be made configurable in the future
    tls_settings.set_min_proto_version(Some(pingora::tls::ssl::SslVersion::TLS1_2))?;
    tls_settings.set_max_proto_version(Some(pingora::tls::ssl::SslVersion::TLS1_3))?;

    service.add_tls_with_settings(https_address, None, tls_settings);
    Ok(service)
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
