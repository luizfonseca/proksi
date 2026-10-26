//! Metrics of requests and upstreams (ADR 0004), exposed in the Prometheus text format.
//!
//! Labels come from the configuration (host name, location path, upstream address), never from
//! requests. A location's metrics are created when it is built, so recording a request only
//! increments atomics.

pub mod live;
pub mod process;
pub mod requests;

use std::time::Duration;

use once_cell::sync::Lazy;
use prometheus::{
    Histogram, HistogramVec, IntCounter, IntCounterVec, IntGauge, IntGaugeVec, Registry,
};

static ENABLED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Records metrics from now on (with the `admin` block, unless `metrics = false`). Otherwise
/// nothing is recorded: locations and `tcp` blocks get no metrics.
pub fn enable() {
    ENABLED.store(true, std::sync::atomic::Ordering::Relaxed);
}

pub fn enabled() -> bool {
    ENABLED.load(std::sync::atomic::Ordering::Relaxed)
}

/// Why no location served a request, as its host label: a fixed few, so clients can't create
/// new series by sending new names or paths
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Unserved {
    /// No host has the request's name
    #[default]
    UnknownHost,
    /// A host has the name, but none of its locations the path
    NoLocation,
    /// Redirected to HTTPS
    HttpsRedirect,
    /// A Let's Encrypt challenge
    Acme,
    /// Answered by the server's or a host's rules, before a location was chosen
    Rules,
}

impl Unserved {
    const ALL: [Self; 5] = [
        Self::UnknownHost,
        Self::NoLocation,
        Self::HttpsRedirect,
        Self::Acme,
        Self::Rules,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::UnknownHost => "_unknown_host",
            Self::NoLocation => "_no_location",
            Self::HttpsRedirect => "_https_redirect",
            Self::Acme => "_acme",
            Self::Rules => "_rules",
        }
    }
}

/// Status classes, by the first digit of the status (`none`: no response was sent)
const STATUS_CLASSES: [&str; 6] = ["none", "1xx", "2xx", "3xx", "4xx", "5xx"];

/// Latency buckets, from 0.5 ms to 10 s (fine enough to read percentiles from)
pub const BUCKETS: [f64; 14] = [
    0.0005, 0.001, 0.0025, 0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0, 2.5, 5.0, 10.0,
];

struct Metrics {
    registry: Registry,
    requests: IntCounterVec,
    responses: IntCounterVec,
    request_duration: HistogramVec,
    upstream_duration: HistogramVec,
    upstream_errors: IntCounterVec,
    upstream_healthy: IntGaugeVec,
    blocked: IntCounterVec,
    tcp_connections: IntCounterVec,
    tcp_active: IntGaugeVec,
    tcp_bytes: IntCounterVec,
    tcp_upstream_errors: IntCounterVec,
    tls_handshake_failures: IntCounterVec,
    notifications: IntCounterVec,
    tcp_duration: HistogramVec,
    certificate_expiry: IntGaugeVec,
    routes: IntGaugeVec,
}

static METRICS: Lazy<Metrics> = Lazy::new(|| {
    let registry = Registry::new();
    let counter = |name: &str, help: &str, labels: &[&str]| {
        let counter = IntCounterVec::new(prometheus::Opts::new(name, help), labels)
            .unwrap_or_else(|err| unreachable!("invalid metric {name}: {err}"));
        let _ = registry.register(Box::new(counter.clone()));
        counter
    };
    let histogram = |name: &str, help: &str, labels: &[&str]| {
        let opts = prometheus::HistogramOpts::new(name, help).buckets(BUCKETS.to_vec());
        let histogram = HistogramVec::new(opts, labels)
            .unwrap_or_else(|err| unreachable!("invalid metric {name}: {err}"));
        let _ = registry.register(Box::new(histogram.clone()));
        histogram
    };
    let gauge = |name: &str, help: &str, labels: &[&str]| {
        let gauge = IntGaugeVec::new(prometheus::Opts::new(name, help), labels)
            .unwrap_or_else(|err| unreachable!("invalid metric {name}: {err}"));
        let _ = registry.register(Box::new(gauge.clone()));
        gauge
    };

    Metrics {
        requests: counter(
            "proksi_requests_total",
            "Requests, by status class",
            &["host", "location", "status"],
        ),
        responses: counter(
            "proksi_responses_total",
            "Responses, by status code",
            &["host", "location", "code"],
        ),
        request_duration: histogram(
            "proksi_request_duration_seconds",
            "Time to answer requests",
            &["host", "location"],
        ),
        upstream_duration: histogram(
            "proksi_upstream_duration_seconds",
            "Time until the upstream's response headers",
            &["host", "location", "upstream"],
        ),
        upstream_errors: counter(
            "proksi_upstream_errors_total",
            "Failures to connect to an upstream, or while proxying to it",
            &["host", "location", "upstream"],
        ),
        upstream_healthy: gauge(
            "proksi_upstream_healthy",
            "Whether the health checks consider the upstream healthy (1) or not (0)",
            &["host", "location", "upstream"],
        ),
        blocked: counter(
            "proksi_blocked_requests_total",
            "Requests answered by a building block (rate_limit, body_limit, auth...)",
            &["host", "location", "block"],
        ),
        tcp_connections: counter(
            "proksi_tcp_connections_total",
            "Connections accepted by a tcp block",
            &["block"],
        ),
        tcp_active: gauge(
            "proksi_tcp_connections_active",
            "Open connections of a tcp block",
            &["block"],
        ),
        tcp_bytes: counter(
            "proksi_tcp_bytes_total",
            "Bytes forwarded by a tcp block, from the client (in) and to it (out)",
            &["block", "direction"],
        ),
        tcp_upstream_errors: counter(
            "proksi_tcp_upstream_errors_total",
            "Connections of a tcp block that couldn't reach an upstream (refused, timed out, or \
             none available)",
            &["block"],
        ),
        notifications: counter(
            "proksi_notifications_total",
            "Events for notify webhooks: sent, failed (after retries), or dropped (too many at once)",
            &["notify", "event", "result"],
        ),
        tls_handshake_failures: {
            let failures = counter(
                "proksi_tls_handshake_failures_total",
                "TLS handshakes with clients that started and didn't complete",
                &[],
            );
            failures.with_label_values::<&str>(&[]);
            failures
        },
        tcp_duration: histogram(
            "proksi_tcp_connection_duration_seconds",
            "How long connections of a tcp block stay open",
            &["block"],
        ),
        certificate_expiry: gauge(
            "proksi_certificate_expiry_timestamp_seconds",
            "When the certificate of a domain expires",
            &["domain"],
        ),
        routes: gauge("proksi_routes", "Hosts applied, by source", &["source"]),
        registry,
    }
});

/// The metrics of one location (clones share the same series)
#[derive(Clone)]
pub struct LocationMetrics {
    host: String,
    location: String,
    requests: [IntCounter; 6],
    /// By status code, created as codes are first answered
    responses: std::sync::Arc<std::sync::RwLock<Vec<(u16, IntCounter)>>>,
    duration: Histogram,
    upstreams: Vec<UpstreamMetrics>,
    blocked: Vec<(&'static str, IntCounter)>,
}

/// The metrics of one upstream of a location
#[derive(Clone)]
pub struct UpstreamMetrics {
    duration: Histogram,
    errors: IntCounter,
    healthy: IntGauge,
}

impl LocationMetrics {
    /// The metrics of location `location` of `host`, with one set per upstream (labelled by
    /// their address, in the order of the location's upstreams)
    pub fn new(host: &str, location: &str, upstreams: &[String]) -> Self {
        Self::with_blocks(host, location, upstreams, &[])
    }

    /// Like `new`, with a counter for each building block of the location (`rate_limit`...)
    pub fn with_blocks(
        host: &str,
        location: &str,
        upstreams: &[String],
        blocks: &[&'static str],
    ) -> Self {
        let metrics = &*METRICS;
        Self {
            host: host.to_string(),
            location: location.to_string(),
            responses: std::sync::Arc::default(),
            requests: STATUS_CLASSES.map(|status| {
                metrics
                    .requests
                    .with_label_values(&[host, location, status])
            }),
            duration: metrics
                .request_duration
                .with_label_values(&[host, location]),
            upstreams: upstreams
                .iter()
                .map(|upstream| UpstreamMetrics {
                    duration: metrics
                        .upstream_duration
                        .with_label_values(&[host, location, upstream]),
                    errors: metrics
                        .upstream_errors
                        .with_label_values(&[host, location, upstream]),
                    healthy: {
                        // Upstreams start healthy, until a health check fails
                        let healthy = metrics
                            .upstream_healthy
                            .with_label_values(&[host, location, upstream]);
                        healthy.set(1);
                        healthy
                    },
                })
                .collect(),
            blocked: blocks
                .iter()
                .map(|block| {
                    (
                        *block,
                        metrics.blocked.with_label_values(&[host, location, block]),
                    )
                })
                .collect(),
        }
    }

    /// Requests no location served, by why
    pub fn unserved(why: Unserved) -> &'static Self {
        static UNSERVED: Lazy<[LocationMetrics; 5]> =
            Lazy::new(|| Unserved::ALL.map(|why| LocationMetrics::new(why.label(), "", &[])));
        let [unknown_host, no_location, https_redirect, acme, rules] = &*UNSERVED;
        match why {
            Unserved::UnknownHost => unknown_host,
            Unserved::NoLocation => no_location,
            Unserved::HttpsRedirect => https_redirect,
            Unserved::Acme => acme,
            Unserved::Rules => rules,
        }
    }

    /// Records a request answered with `status` (0 if no response was sent) after `duration`
    pub fn record(&self, status: u16, duration: Duration) {
        let class = match status {
            100..=599 => usize::from(status / 100),
            _ => 0,
        };
        if let Some(requests) = self.requests.get(class) {
            requests.inc();
        }
        if class > 0 {
            self.count_response(status);
        }
        self.duration.observe(duration.as_secs_f64());
    }

    /// Counts a response with `code`: a location answers few codes, kept in a list
    fn count_response(&self, code: u16) {
        let responses = self
            .responses
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some((_, counter)) = responses.iter().find(|(known, _)| *known == code) {
            counter.inc();
            return;
        }
        drop(responses);

        let mut responses = self
            .responses
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if !responses.iter().any(|(known, _)| *known == code) {
            let counter = METRICS.responses.with_label_values(&[
                self.host.as_str(),
                self.location.as_str(),
                &code.to_string(),
            ]);
            responses.push((code, counter));
        }
        if let Some((_, counter)) = responses.iter().find(|(known, _)| *known == code) {
            counter.inc();
        }
    }

    /// The metrics of the upstream at `index` in the location's upstreams
    pub fn upstream(&self, index: usize) -> Option<&UpstreamMetrics> {
        self.upstreams.get(index)
    }

    /// A request answered by a building block (`rate_limit`, `basic_auth`...)
    pub fn blocked(&self, block: &str) {
        if let Some((_, counter)) = self.blocked.iter().find(|(name, _)| *name == block) {
            counter.inc();
        }
    }
}

impl UpstreamMetrics {
    /// Time until the upstream's response headers
    pub fn record(&self, duration: Duration) {
        self.duration.observe(duration.as_secs_f64());
    }

    /// Failed to connect, or the connection failed while proxying
    pub fn error(&self) {
        self.errors.inc();
    }

    /// Whether the health checks consider the upstream healthy
    pub fn set_healthy(&self, healthy: bool) {
        self.healthy.set(i64::from(healthy));
    }
}

/// The metrics of a `tcp` block
#[derive(Clone)]
pub struct TcpMetrics {
    connections: IntCounter,
    active: IntGauge,
    /// Bytes from the client to the upstream, and back
    pub bytes_in: IntCounter,
    pub bytes_out: IntCounter,
    duration: Histogram,
    upstream_errors: IntCounter,
}

/// An open TCP connection: counted as active until dropped, when its duration is recorded
pub struct TcpConnection {
    metrics: TcpMetrics,
    start: std::time::Instant,
}

impl TcpMetrics {
    /// The metrics of the block labelled `block` (`tcp "db" in host "a.com"`)
    pub fn new(block: &str) -> Self {
        let metrics = &*METRICS;
        Self {
            connections: metrics.tcp_connections.with_label_values(&[block]),
            active: metrics.tcp_active.with_label_values(&[block]),
            bytes_in: metrics.tcp_bytes.with_label_values(&[block, "in"]),
            bytes_out: metrics.tcp_bytes.with_label_values(&[block, "out"]),
            duration: metrics.tcp_duration.with_label_values(&[block]),
            upstream_errors: metrics.tcp_upstream_errors.with_label_values(&[block]),
        }
    }

    /// A connection that couldn't reach an upstream
    pub fn upstream_error(&self) {
        self.upstream_errors.inc();
    }

    /// A new connection
    pub fn connection(&self) -> TcpConnection {
        self.connections.inc();
        self.active.inc();
        TcpConnection {
            metrics: self.clone(),
            start: std::time::Instant::now(),
        }
    }
}

impl Drop for TcpConnection {
    fn drop(&mut self) {
        self.metrics.active.dec();
        self.metrics
            .duration
            .observe(self.start.elapsed().as_secs_f64());
    }
}

/// Keeps `proksi_certificate_expiry_timestamp_seconds` up to date
pub async fn certificate_expiry() {
    // Once hosts (and their certificates) had time to load, then every minute
    let mut interval = tokio::time::interval_at(
        tokio::time::Instant::now() + Duration::from_secs(5),
        Duration::from_secs(60),
    );
    loop {
        interval.tick().await;
        let expiry = crate::stores::global::get_store()
            .get_certificates()
            .await
            .iter()
            .filter_map(|(domain, certificate)| {
                expiry_timestamp(certificate).map(|expiry| (domain.clone(), expiry))
            })
            .collect::<Vec<_>>();
        set_certificates(&expiry);
    }
}

/// When each domain's certificate expires (Unix timestamp), replacing the previous ones
pub fn set_certificates(expiry: &[(String, i64)]) {
    if !enabled() {
        return;
    }
    let gauge = &METRICS.certificate_expiry;
    gauge.reset();
    for (domain, timestamp) in expiry {
        gauge.with_label_values(&[domain]).set(*timestamp);
    }
}

/// A TLS handshake with a client that didn't complete
/// An event for the `notify` block `notify`: `sent`, `failed` or `dropped`
pub fn notification(notify: &str, event: &str, result: &str) {
    if enabled() {
        METRICS
            .notifications
            .with_label_values(&[notify, event, result])
            .inc();
    }
}

pub fn tls_handshake_failed() {
    if enabled() {
        METRICS
            .tls_handshake_failures
            .with_label_values::<&str>(&[])
            .inc();
    }
}

/// Hosts applied from each source
pub fn set_routes(source: &str, hosts: usize) {
    if !enabled() {
        return;
    }
    METRICS
        .routes
        .with_label_values(&[source])
        .set(i64::try_from(hosts).unwrap_or(i64::MAX));
}

/// Every metric, as gathered by the registry
fn families() -> Vec<prometheus::proto::MetricFamily> {
    METRICS.registry.gather()
}

/// Every metric, in the Prometheus text format
pub fn gather() -> String {
    prometheus::TextEncoder::new()
        .encode_to_string(&METRICS.registry.gather())
        .unwrap_or_else(|err| format!("# failed to encode metrics: {err}\n"))
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::{gather, LocationMetrics, Unserved};

    fn line<'a>(text: &'a str, prefix: &str) -> Option<&'a str> {
        text.lines().find(|line| line.starts_with(prefix))
    }

    #[test]
    fn test_requests_by_status_class() {
        let metrics = LocationMetrics::new("metrics-a.test", "/api", &[]);
        metrics.record(200, Duration::from_millis(3));
        metrics.record(204, Duration::from_millis(3));
        metrics.record(404, Duration::from_millis(3));
        metrics.record(0, Duration::from_millis(3));

        let text = gather();
        let requests = |status: &str| {
            line(
                &text,
                &format!(
                    "proksi_requests_total{{host=\"metrics-a.test\",location=\"/api\",status=\"{status}\"}}"
                ),
            )
            .map(|line| line.rsplit(' ').next().unwrap())
        };
        assert_eq!(requests("2xx"), Some("2"));
        assert_eq!(requests("4xx"), Some("1"));
        assert_eq!(requests("none"), Some("1"));
        assert_eq!(
            line(
                &text,
                "proksi_request_duration_seconds_count{host=\"metrics-a.test\",location=\"/api\"}"
            ),
            Some("proksi_request_duration_seconds_count{host=\"metrics-a.test\",location=\"/api\"} 4")
        );
    }

    #[test]
    fn test_a_rebuilt_location_continues_its_series() {
        LocationMetrics::new("metrics-b.test", "/", &[]).record(200, Duration::from_millis(1));
        LocationMetrics::new("metrics-b.test", "/", &[]).record(200, Duration::from_millis(1));

        assert!(gather().contains(
            "proksi_requests_total{host=\"metrics-b.test\",location=\"/\",status=\"2xx\"} 2"
        ));
    }

    #[test]
    fn test_upstreams() {
        let metrics = LocationMetrics::new(
            "metrics-c.test",
            "/",
            &["10.0.0.1:80".to_string(), "app:8080".to_string()],
        );
        let upstream = metrics.upstream(1).unwrap();
        upstream.record(Duration::from_millis(20));
        upstream.error();
        assert!(metrics.upstream(2).is_none());

        let text = gather();
        assert!(text.contains(
            "proksi_upstream_duration_seconds_count{host=\"metrics-c.test\",location=\"/\",upstream=\"app:8080\"} 1"
        ));
        assert!(text.contains(
            "proksi_upstream_errors_total{host=\"metrics-c.test\",location=\"/\",upstream=\"app:8080\"} 1"
        ));
    }

    #[test]
    fn test_requests_no_location_served() {
        for why in Unserved::ALL {
            assert_eq!(LocationMetrics::unserved(why).host, why.label());
        }
        LocationMetrics::unserved(Unserved::HttpsRedirect).record(301, Duration::from_millis(1));

        assert!(gather().contains(
            "proksi_requests_total{host=\"_https_redirect\",location=\"\",status=\"3xx\"}"
        ));
    }

    #[test]
    fn test_responses_by_status_code() {
        let metrics = LocationMetrics::new("metrics-codes.test", "/", &[]);
        metrics.record(200, Duration::from_millis(1));
        metrics.record(429, Duration::from_millis(1));
        metrics.record(429, Duration::from_millis(1));
        // No response was sent
        metrics.record(0, Duration::from_millis(1));

        let text = gather();
        let code = |code: &str| {
            format!("proksi_responses_total{{code=\"{code}\",host=\"metrics-codes.test\",location=\"/\"}}")
        };
        assert!(text.contains(&format!("{} 1", code("200"))), "{text}");
        assert!(text.contains(&format!("{} 2", code("429"))), "{text}");
        assert!(!text.contains(&code("0")), "{text}");
        // By class too
        assert!(text.contains(
            "proksi_requests_total{host=\"metrics-codes.test\",location=\"/\",status=\"4xx\"} 2"
        ));
    }

    #[test]
    fn test_blocked_requests() {
        let metrics =
            LocationMetrics::with_blocks("metrics-d.test", "/", &[], &["rate_limit", "basic_auth"]);
        metrics.blocked("rate_limit");
        metrics.blocked("rate_limit");
        // Blocks the location doesn't have are ignored
        metrics.blocked("oauth2");

        let text = gather();
        assert!(text.contains(
            "proksi_blocked_requests_total{block=\"rate_limit\",host=\"metrics-d.test\",location=\"/\"} 2"
        ), "{text}");
        assert!(text.contains(
            "proksi_blocked_requests_total{block=\"basic_auth\",host=\"metrics-d.test\",location=\"/\"} 0"
        ));
        assert!(!text.contains("block=\"oauth2\",host=\"metrics-d.test\""));
    }

    #[test]
    fn test_upstream_health() {
        let metrics = LocationMetrics::new("metrics-e.test", "/", &["10.0.0.1:80".to_string()]);
        let healthy = || {
            gather().contains(
                "proksi_upstream_healthy{host=\"metrics-e.test\",location=\"/\",upstream=\"10.0.0.1:80\"} 1",
            )
        };
        // Upstreams start healthy
        assert!(healthy());
        metrics.upstream(0).unwrap().set_healthy(false);
        assert!(!healthy());
        metrics.upstream(0).unwrap().set_healthy(true);
        assert!(healthy());
    }

    #[test]
    fn test_tcp_connections() {
        let metrics = super::TcpMetrics::new("tcp \"metrics-f\"");
        let label = "block=\"tcp \\\"metrics-f\\\"\"";
        let connection = metrics.connection();
        metrics.bytes_in.inc_by(10);
        metrics.bytes_out.inc_by(25);

        let text = gather();
        assert!(
            text.contains(&format!("proksi_tcp_connections_total{{{label}}} 1")),
            "{text}"
        );
        assert!(text.contains(&format!("proksi_tcp_connections_active{{{label}}} 1")));
        assert!(text.contains(&format!(
            "proksi_tcp_bytes_total{{{label},direction=\"in\"}} 10"
        )));
        assert!(text.contains(&format!(
            "proksi_tcp_bytes_total{{{label},direction=\"out\"}} 25"
        )));

        drop(connection);
        let text = gather();
        assert!(text.contains(&format!("proksi_tcp_connections_active{{{label}}} 0")));
        assert!(text.contains(&format!(
            "proksi_tcp_connection_duration_seconds_count{{{label}}} 1"
        )));
    }

    #[test]
    fn test_certificates_and_routes() {
        crate::metrics::enable();
        super::set_certificates(&[
            ("metrics-g.test".to_string(), 1_800_000_000),
            ("metrics-h.test".to_string(), 1_900_000_000),
        ]);
        super::set_certificates(&[("metrics-g.test".to_string(), 1_850_000_000)]);
        super::set_routes("file", 3);

        let text = gather();
        assert!(text.contains(
            "proksi_certificate_expiry_timestamp_seconds{domain=\"metrics-g.test\"} 1850000000"
        ));
        // Removed certificates are dropped
        assert!(!text.contains("metrics-h.test"));
        assert!(text.contains("proksi_routes{source=\"file\"} 3"));
    }
}

/// When `certificate` expires, as a Unix timestamp
pub fn expiry_timestamp(certificate: &proksi_store::Certificate) -> Option<i64> {
    let epoch = openssl::asn1::Asn1Time::from_unix(0).ok()?;
    let diff = epoch.diff(certificate.leaf.not_after()).ok()?;
    Some(i64::from(diff.days) * 86_400 + i64::from(diff.secs))
}

#[cfg(test)]
mod expiry_tests {
    #[test]
    fn test_expiry_timestamp() {
        let certificate = proksi_store::test_util::certificate("a.test", 30);
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let expected = i64::try_from(now + 30 * 86_400).unwrap();

        let expiry = super::expiry_timestamp(&certificate).unwrap();

        assert!((expiry - expected).abs() < 5, "{expiry} != {expected}");
    }
}

#[cfg(test)]
mod overhead {
    use std::time::{Duration, Instant};

    use super::LocationMetrics;

    /// What recording costs per request: `cargo test --release -- --ignored metrics_overhead`
    #[test]
    #[ignore]
    fn metrics_overhead() {
        let metrics = LocationMetrics::new("overhead.test", "/", &["10.0.0.1:80".to_string()]);
        let per_request = |metrics: &LocationMetrics, requests: u32| {
            let start = Instant::now();
            for i in 0..requests {
                let upstream = metrics.upstream(0).unwrap();
                upstream.record(Duration::from_micros(u64::from(i % 5000)));
                metrics.record(200, Duration::from_micros(u64::from(i % 9000)));
            }
            start.elapsed() / requests
        };

        println!(
            "1 thread: {:?} per request",
            per_request(&metrics, 10_000_000)
        );
        let threads = (0..8)
            .map(|_| {
                let metrics = metrics.clone();
                std::thread::spawn(move || per_request(&metrics, 2_000_000))
            })
            .collect::<Vec<_>>();
        let times = threads
            .into_iter()
            .map(|t| t.join().unwrap())
            .collect::<Vec<_>>();
        println!("8 threads, same location: {times:?} per request");
    }
}

#[cfg(test)]
mod overhead_parts {
    use std::time::Instant;

    /// `cargo test --release -- --ignored metrics_overhead_parts`
    #[test]
    #[ignore]
    fn metrics_overhead_parts() {
        let counter = prometheus::IntCounter::new("c", "c").unwrap();
        let histogram = prometheus::Histogram::with_opts(
            prometheus::HistogramOpts::new("h", "h").buckets(super::BUCKETS.to_vec()),
        )
        .unwrap();
        let contended = |work: std::sync::Arc<dyn Fn(u32) + Send + Sync>| {
            let threads = (0..8)
                .map(|_| {
                    let work = work.clone();
                    std::thread::spawn(move || {
                        let start = Instant::now();
                        for i in 0..2_000_000 {
                            work(i);
                        }
                        start.elapsed() / 2_000_000
                    })
                })
                .collect::<Vec<_>>();
            threads
                .into_iter()
                .map(|t| t.join().unwrap())
                .max()
                .unwrap()
        };
        let c = counter.clone();
        println!(
            "counter, 8 threads: {:?}",
            contended(std::sync::Arc::new(move |_| c.inc()))
        );
        let h = histogram.clone();
        println!(
            "histogram, 8 threads: {:?}",
            contended(std::sync::Arc::new(
                move |i| h.observe(f64::from(i % 9000) / 1e6)
            ))
        );
        let local = std::sync::Arc::new(histogram.clone());
        println!(
            "thread-local histogram, 8 threads: {:?}",
            contended(std::sync::Arc::new(move |i| {
                thread_local! {
                    static LOCAL: std::cell::RefCell<Option<prometheus::local::LocalHistogram>> = const { std::cell::RefCell::new(None) };
                }
                LOCAL.with(|local_histogram| {
                    let mut local_histogram = local_histogram.borrow_mut();
                    let h = local_histogram.get_or_insert_with(|| local.local());
                    h.observe(f64::from(i % 9000) / 1e6);
                    if i % 1024 == 0 {
                        h.flush();
                    }
                });
            }))
        );
    }
}
