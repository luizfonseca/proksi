// Paths are formatted with `{:?}` on purpose, see `disallowed-methods` in clippy.toml
#![allow(clippy::unnecessary_debug_formatting)]

use ::pingora::{server::Server, services::ServiceHandle};

use clap::crate_version;
use config::{load, LogFormat, LogLevel};
use proksi_store::MemoryStore;
use stores::global::init_store;
use tracing_subscriber::EnvFilter;

use std::sync::{atomic::AtomicU64, Arc};

use pingora::server::configuration::{Opt, ServerConf};

use services::{logger::LoggerService, BackgroundFunctionService};

use proksi_plugins::ip_set;

mod admin;
#[cfg(test)]
mod allocations;
mod blocks;
mod cache;
mod channel;
mod config;
mod docker;
mod health_check;
#[cfg(feature = "kubernetes")]
mod kubernetes;
mod letsencrypt;
mod logger;
mod metrics;
mod notify;
mod proxy_server;
mod routing;
mod rules;
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
#[cfg(not(test))]
#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

// The same, counting the allocations of the hot path's tests
#[cfg(test)]
#[global_allocator]
static GLOBAL: allocations::Counting = allocations::Counting;

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
    // Transparent huge pages back mimalloc's memory with 2 MiB pages where the system enables
    // them for every process (`always`, common on servers), more than doubling what a worker
    // keeps. Turned off for this process and the workers it starts.
    #[cfg(target_os = "linux")]
    let _ = nix::sys::prctl::set_thp_disable(true);

    // Linux: a supervisor starts the worker serving requests, and replaces it without dropping
    // connections when settings change (see `supervisor`). It's started before anything else
    // loads, so it holds only what supervising needs.
    #[cfg(target_os = "linux")]
    if supervisor::should_supervise() {
        std::process::exit(supervisor::run()?);
    }
    #[cfg(target_os = "linux")]
    supervisor::restore_reservations();

    // Configuration can be refreshed on file change
    // Loads configuration from command-line, YAML or TOML sources
    let proxy_config = Arc::new(
        load("/etc/proksi/configs")
            .map_err(|err| anyhow::anyhow!("Failed to load configuration: {err}"))?,
    );

    // `daemon = true` in the configuration file: this worker runs on its own in the background
    #[cfg(target_os = "linux")]
    if proxy_config.daemon {
        supervisor::detach();
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
    let (log_sender, log_receiver) = logger::log_queue();
    let dropped_logs = Arc::new(AtomicU64::new(0));

    // Route sources send their routes to the reconciler through this channel
    let (route_sender, route_receiver) = routing::service::snapshot_channel();
    let appender = logger::ProxyLog::new(
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
            init_store(MemoryStore::new().with_history(proxy_config.store.history_per_host));
        }
        config::StoreType::Redis => {
            let redis_url = proxy_config.store.redis_url.as_deref().ok_or_else(|| {
                anyhow::anyhow!(
                    "Failed to get redis_url from configuration when store type is 'redis'"
                )
            })?;
            let redis_store = proksi_store::RedisStore::new(redis_url)
                .map_err(|err| anyhow::anyhow!("Failed to initialize Redis store: {err}"))?
                .with_history(proxy_config.store.history_per_host);
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
    server_conf.work_stealing = proxy_config.server.work_stealing();
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
    pingora_server.add_service(LoggerService::new(logger::ProxyLoggerReceiver::new(
        log_receiver,
        dropped_logs,
        &proxy_config,
    )));

    set_server_settings(&proxy_config.server)?;
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

/// The `server` settings requests read: timeouts, TLS, and the rules of every request (validated
/// with the configuration)
fn set_server_settings(server: &config::ServerCfg) -> Result<(), anyhow::Error> {
    proxy_server::timeouts::set_client_timeouts(server.timeouts);
    proxy_server::services::set_server_tls(server.tls.clone());
    rules::set_server_rules(&server.rule).map_err(anyhow::Error::msg)
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
        // What only the admin API reads is kept only with it (routes are built after this)
        admin::enable();
        let (tokens, trusted_origins) = proxy_config
            .admin
            .as_ref()
            .map(|admin| (admin.tokens.clone(), admin.trusted_origins.clone()))
            .unwrap_or_default();
        let instance = admin::status::Instance {
            started: std::time::Instant::now(),
            store_type: format!("{:?}", proxy_config.store.store_type).to_lowercase(),
            listeners: serde_json::json!({
                "https": listeners.https,
                "http": listeners.http,
                "tcp": listeners.tcp.iter().map(|route| route.listen.clone()).collect::<std::collections::BTreeSet<_>>(),
                "admin": address.to_string(),
                "metrics": listeners.metrics.map(|address| address.to_string()),
            }),
            server: Some(admin::server::Settings::new(proxy_config)),
            tcp: listeners.tcp.clone(),
        };
        handles.push(server.add_service(admin::admin_service(address, tokens, trusted_origins, instance)));
    }
    if let Some(address) = listeners.metrics {
        let token = proxy_config
            .admin
            .as_ref()
            .and_then(|admin| admin.metrics_token().map(ToString::to_string));
        handles.push(server.add_service(admin::metrics_service(address, token)));
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
fn init_tracing(proxy_config: &config::Config, appender: logger::ProxyLog) {
    use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

    logger::set_access_logs(
        proxy_config.logging.enabled && proxy_config.logging.access_logs_enabled,
    );
    let level = &proxy_config.logging.level;
    let handshake_errors = logger::handshake_error_filter::HandshakeErrorFilter {
        debug: matches!(level, LogLevel::Debug | LogLevel::Trace),
    };
    if proxy_config.logging.format == LogFormat::Json {
        tracing_subscriber::fmt()
            .json()
            .with_env_filter(EnvFilter::from_default_env())
            .with_max_level(level)
            .with_writer(appender)
            .finish()
            .with(handshake_errors)
            .init();
    } else {
        tracing_subscriber::fmt()
            .with_env_filter(EnvFilter::from_default_env())
            .with_max_level(level)
            .with_ansi(proxy_config.logging.path.is_none())
            .with_writer(appender)
            .finish()
            .with(handshake_errors)
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
        let sql_store = proksi_store::SqlStore::connect(database_url)
            .map_err(|err| anyhow::anyhow!("Failed to initialize the SQL store: {err}"))?
            .with_history(store.history_per_host);
        tracing::info!("using {:?} store", store.store_type);
        init_store(sql_store);
    }

    Ok(())
}
