//! Metrics of requests and upstreams (ADR 0004), exposed in the Prometheus text format.
//!
//! Labels come from the configuration (host name, location path, upstream address), never from
//! requests. A location's metrics are created when it is built, so recording a request only
//! increments atomics.

use std::time::Duration;

use once_cell::sync::Lazy;
use prometheus::{
    Histogram, HistogramVec, IntCounter, IntCounterVec, IntGauge, IntGaugeVec, Registry,
};

/// Host label of requests that match no host
pub const UNKNOWN: &str = "_unknown";

/// Status classes, by the first digit of the status (`none`: no response was sent)
const STATUS_CLASSES: [&str; 6] = ["none", "1xx", "2xx", "3xx", "4xx", "5xx"];

/// Latency buckets, from 0.5 ms to 10 s (fine enough to read percentiles from)
pub const BUCKETS: [f64; 14] = [
    0.0005, 0.001, 0.0025, 0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0, 2.5, 5.0, 10.0,
];

struct Metrics {
    registry: Registry,
    requests: IntCounterVec,
    request_duration: HistogramVec,
    upstream_duration: HistogramVec,
    upstream_errors: IntCounterVec,
    upstream_healthy: IntGaugeVec,
    blocked: IntCounterVec,
    tcp_connections: IntCounterVec,
    tcp_active: IntGaugeVec,
    tcp_bytes: IntCounterVec,
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
    requests: [IntCounter; 6],
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
            requests: STATUS_CLASSES
                .map(|status| metrics.requests.with_label_values(&[host, location, status])),
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

    /// Requests that match no host (or no location of their host)
    pub fn unknown() -> &'static Self {
        static UNKNOWN_METRICS: Lazy<LocationMetrics> =
            Lazy::new(|| LocationMetrics::new(UNKNOWN, "", &[]));
        &UNKNOWN_METRICS
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
        self.duration.observe(duration.as_secs_f64());
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
        }
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

/// When each domain's certificate expires (Unix timestamp), replacing the previous ones
pub fn set_certificates(expiry: &[(String, i64)]) {
    let gauge = &METRICS.certificate_expiry;
    gauge.reset();
    for (domain, timestamp) in expiry {
        gauge.with_label_values(&[domain]).set(*timestamp);
    }
}

/// Hosts applied from each source
pub fn set_routes(source: &str, hosts: usize) {
    METRICS
        .routes
        .with_label_values(&[source])
        .set(i64::try_from(hosts).unwrap_or(i64::MAX));
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

    use super::{gather, LocationMetrics};

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
            .map(|line| line.rsplit(' ').next().unwrap().to_string())
        };
        assert_eq!(requests("2xx").as_deref(), Some("2"));
        assert_eq!(requests("4xx").as_deref(), Some("1"));
        assert_eq!(requests("none").as_deref(), Some("1"));
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
    fn test_unknown_hosts() {
        LocationMetrics::unknown().record(404, Duration::from_millis(1));

        assert!(gather()
            .contains("proksi_requests_total{host=\"_unknown\",location=\"\",status=\"4xx\"}"));
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
        assert!(text.contains(&format!("proksi_tcp_connections_total{{{label}}} 1")), "{text}");
        assert!(text.contains(&format!("proksi_tcp_connections_active{{{label}}} 1")));
        assert!(text.contains(&format!("proksi_tcp_bytes_total{{{label},direction=\"in\"}} 10")));
        assert!(text.contains(&format!("proksi_tcp_bytes_total{{{label},direction=\"out\"}} 25")));

        drop(connection);
        let text = gather();
        assert!(text.contains(&format!("proksi_tcp_connections_active{{{label}}} 0")));
        assert!(text.contains(&format!("proksi_tcp_connection_duration_seconds_count{{{label}}} 1")));
    }

    #[test]
    fn test_certificates_and_routes() {
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
pub fn expiry_timestamp(certificate: &crate::stores::certificates::Certificate) -> Option<i64> {
    let epoch = openssl::asn1::Asn1Time::from_unix(0).ok()?;
    let diff = epoch.diff(certificate.leaf.not_after()).ok()?;
    Some(i64::from(diff.days) * 86_400 + i64::from(diff.secs))
}

#[cfg(test)]
mod expiry_tests {
    #[test]
    fn test_expiry_timestamp() {
        let certificate = crate::stores::contract_tests::certificate("a.test", 30);
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let expected = i64::try_from(now + 30 * 86_400).unwrap();

        let expiry = super::expiry_timestamp(&certificate).unwrap();

        assert!((expiry - expected).abs() < 5, "{expiry} != {expected}");
    }
}
