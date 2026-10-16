//! Metrics of requests and upstreams (ADR 0004), exposed in the Prometheus text format.
//!
//! Labels come from the configuration (host name, location path, upstream address), never from
//! requests. A location's metrics are created when it is built, so recording a request only
//! increments atomics.

use std::time::Duration;

use once_cell::sync::Lazy;
use prometheus::{Histogram, HistogramVec, IntCounter, IntCounterVec, Registry};

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
        registry,
    }
});

/// The metrics of one location (clones share the same series)
#[derive(Clone)]
pub struct LocationMetrics {
    requests: [IntCounter; 6],
    duration: Histogram,
    upstreams: Vec<UpstreamMetrics>,
}

/// The metrics of one upstream of a location
#[derive(Clone)]
pub struct UpstreamMetrics {
    duration: Histogram,
    errors: IntCounter,
}

impl LocationMetrics {
    /// The metrics of location `location` of `host`, with one set per upstream (labelled by
    /// their address, in the order of the location's upstreams)
    pub fn new(host: &str, location: &str, upstreams: &[String]) -> Self {
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
}
