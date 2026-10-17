// Paths are formatted with `{:?}` on purpose, see `disallowed-methods` in clippy.toml
#![allow(clippy::unnecessary_debug_formatting)]

use ::pingora::{server::Server, services::ServiceHandle};

use clap::crate_version;
use config::{load, LogFormat};
use stores::{global::init_store, MemoryStore};
use tracing_subscriber::EnvFilter;

use std::sync::{atomic::AtomicU64, Arc};

use pingora::server::configuration::{Opt, ServerConf};

use services::{logger::ProxyLoggerReceiver, BackgroundFunctionService};

mod admin;
mod blocks;
mod cache;
mod channel;
mod config;
#[cfg(feature = "kubernetes")]
mod kubernetes;
mod metrics;
mod plugins;
mod proxy_server;
mod routing;
mod server;
mod services;
mod stores;
#[cfg(target_os = "linux")]
mod supervisor;
mod tcp;
mod tools;
mod wasm;

// Faster than the system allocators (notably musl's) for the many small allocations
// of each request
#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

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

    // Linux: a supervisor starts the worker serving requests, and replaces it without dropping
    // connections when settings change (see `supervisor`)
    #[cfg(target_os = "linux")]
    if !proxy_config.daemon && !supervisor::is_worker() {
        std::process::exit(supervisor::run()?);
    }

    // The addresses the hosts need (validated when loading the configuration)
    let listeners = config::listeners::listeners(&proxy_config).map_err(anyhow::Error::msg)?;
    let (default_https, default_http) =
        config::listeners::default_addresses(&proxy_config).map_err(anyhow::Error::msg)?;

    // Metrics are recorded only for the admin API, unless it turns them off
    if proxy_config.admin.as_ref().is_some_and(|admin| admin.metrics) {
        metrics::enable();
    }

    // The clients of requests behind trusted proxies (validated when loading the configuration)
    proxy_server::client::init(
        proxy_server::client::TrustedProxies::parse(&proxy_config.server.trusted_proxies)
            .map_err(anyhow::Error::msg)?,
    );

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

    // Proksi stops once the connections are closed (`connections::exit_when_drained`): Pingora's
    // grace period only applies if that doesn't happen
    let mut server_conf = ServerConf::new_with_opt_override(&pingora_opts)
        .ok_or_else(|| anyhow::anyhow!("invalid server configuration"))?;
    server_conf.grace_period_seconds = Some(proxy_config.server.shutdown_timeout().as_secs() + 5);
    // Set before the server is created: it's where a new worker waits for the listening sockets
    #[cfg(target_os = "linux")]
    {
        server_conf.upgrade_sock = supervisor::upgrade_socket(&proxy_config);
    }
    let mut pingora_server = Server::new_with_opt_and_conf(pingora_opts, server_conf);
    pingora_server.bootstrap();

    // Add Prometheus service
    // let mut prometheus_service_http = Service::prometheus_http_service();
    // prometheus_service_http.add_tcp("0.0.0.0:9090");
    // pingora_server.add_service(prometheus_service_http);

    // Non-dedicated background services
    // The listening services start once it's ready: routes applied (and, when upgrading,
    // certificates loaded), see `routing::readiness`
    let background = pingora_server.add_service(BackgroundFunctionService::new(
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

    proxy_server::timeouts::set_client_timeouts(proxy_config.server.timeouts);
    for listening in add_listeners(
        &mut pingora_server,
        &proxy_config,
        &listeners,
        (default_https, default_http),
    )? {
        listening.add_dependency(&background);
    }

    pingora_server.run_forever();
}

/// Listens on the HTTP, HTTPS and TCP addresses the hosts and `tcp` blocks need
fn add_listeners(
    server: &mut Server,
    proxy_config: &config::Config,
    listeners: &config::listeners::Listeners,
    (default_https, default_http): (std::net::SocketAddr, std::net::SocketAddr),
) -> Result<Vec<ServiceHandle>, anyhow::Error> {
    let mut handles = vec![];
    // TCP: `tcp` blocks (validated when loading the configuration)
    let tcp_targets = tcp::services::tcp_targets(&listeners.tcp)?;

    // Listen on the HTTP and HTTPS addresses the hosts need
    if !listeners.http.is_empty() {
        handles.push(server.add_service(proxy_server::services::http_service(
            &server.configuration,
            &listeners.http,
            default_http,
            proxy_config.worker_threads,
        )));
    }
    // Addresses with TLS passthrough blocks are served by `tcp::router` (HTTPS included)
    let https = listeners
        .https
        .difference(&tcp::services::passthrough_addresses(&listeners.tcp))
        .cloned()
        .collect::<std::collections::BTreeSet<_>>();
    if !https.is_empty() {
        handles.push(server.add_service(proxy_server::services::https_service(
            &server.configuration,
            &https,
            default_https,
            &tcp_targets,
            proxy_config.worker_threads,
        )?));
    }
    if let Some(address) = listeners.admin {
        let token = proxy_config
            .admin
            .as_ref()
            .and_then(|admin| admin.token().map(ToString::to_string));
        let instance = admin::status::Instance {
            started: std::time::Instant::now(),
            store_type: format!("{:?}", proxy_config.store.store_type).to_lowercase(),
            listeners: serde_json::json!({
                "https": listeners.https,
                "http": listeners.http,
                "tcp": listeners.tcp.iter().map(|route| route.listen.clone()).collect::<std::collections::BTreeSet<_>>(),
                "admin": address.to_string(),
            }),
        };
        handles.push(server.add_service(admin::admin_service(address, token, instance)));
    }
    handles.extend(server.add_services(tcp::services::tcp_services(
        &listeners.tcp,
        &tcp_targets,
        &listeners.https,
        (&server.configuration, default_https),
        proxy_config.worker_threads,
    )?));

    let tcp = listeners
        .tcp
        .iter()
        .map(|route| route.listen.as_str())
        .collect::<std::collections::BTreeSet<_>>();
    let server_info = format!(
        "listening for HTTPS on {:?}, HTTP on {:?} and TCP on {tcp:?}",
        listeners.https, listeners.http
    );
    tracing::info!(
        version = crate_version!(),
        workers = proxy_config.worker_threads,
        server_info,
    );

    Ok(handles)
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
