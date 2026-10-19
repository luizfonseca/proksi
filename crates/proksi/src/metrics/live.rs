//! Live stats (ADR 0004): once a second, the metrics are sampled and kept for 5 minutes. A
//! window is the difference between the latest sample and an earlier one, so the request path
//! does no extra work.
//!
//! Kept small: each series' name and labels (and histogram bounds) are kept once, and a sample
//! holds only numbers. Samples are kept every second for a minute, then every 10 seconds.
//!
//! Sampled only while someone reads the stats: 5 minutes after the last read, sampling stops and
//! the samples are dropped, until the next read.

use std::{
    collections::{BTreeMap, HashMap, VecDeque},
    sync::{Arc, Mutex, PoisonError},
    time::{Duration, Instant},
};

use serde_json::{json, Map, Value};

/// Kept every second for this long, then every `SPARSE`, up to `OLDEST`
const DENSE: Duration = Duration::from_secs(60);
const SPARSE: Duration = Duration::from_secs(10);
const OLDEST: Duration = Duration::from_secs(301);

/// A series: its metric name and labels (sorted by name)
pub type Key = (String, Vec<(String, String)>);

#[derive(Debug, Clone, PartialEq)]
pub enum Reading {
    /// A counter or a gauge
    Scalar(f64),
    /// Cumulative bucket counts, by upper bound (the last one is `+Inf`)
    Histogram(Vec<(f64, u64)>),
}

/// A reading as kept: a histogram's bounds are kept once, with its series
#[derive(Debug, Clone, PartialEq)]
enum Kept {
    Missing,
    Scalar(f64),
    Histogram(Box<[u64]>),
}

/// Every series seen, by id
#[derive(Default)]
struct Series {
    ids: HashMap<Key, usize>,
    keys: Vec<Key>,
    /// Histograms' upper bounds (the last one is `+Inf`)
    bounds: Vec<Arc<[f64]>>,
}

static SERIES: Mutex<Option<Series>> = Mutex::new(None);

/// Every series at one point in time
#[derive(Debug, Clone)]
pub struct Sample {
    pub at: Instant,
    /// By series id
    readings: Vec<Kept>,
    /// The process' CPU time and memory
    pub process: super::process::Reading,
}

impl Sample {
    /// A sample of these readings, at `at`
    pub fn new(at: Instant, series: &[(Key, Reading)], process: super::process::Reading) -> Self {
        let mut known = SERIES.lock().unwrap_or_else(PoisonError::into_inner);
        let known = known.get_or_insert_with(Series::default);
        let mut readings = vec![];
        for (key, reading) in series {
            let id = known.id(key, reading);
            if readings.len() <= id {
                readings.resize(id + 1, Kept::Missing);
            }
            if let Some(slot) = readings.get_mut(id) {
                *slot = match reading {
                    Reading::Scalar(value) => Kept::Scalar(*value),
                    Reading::Histogram(buckets) => {
                        Kept::Histogram(buckets.iter().map(|(_, count)| *count).collect())
                    }
                };
            }
        }
        Self {
            at,
            readings,
            process,
        }
    }

    /// The reading of a series, if it has one
    #[cfg(test)]
    pub fn get(&self, key: &Key) -> Option<Reading> {
        let known = SERIES.lock().unwrap_or_else(PoisonError::into_inner);
        let known = known.as_ref()?;
        let id = *known.ids.get(key)?;
        known.reading(id, self.readings.get(id)?)
    }
}

impl Series {
    /// The id of `key`, given an id when first seen
    fn id(&mut self, key: &Key, reading: &Reading) -> usize {
        if let Some(id) = self.ids.get(key) {
            return *id;
        }
        let id = self.keys.len();
        self.ids.insert(key.clone(), id);
        self.keys.push(key.clone());
        let bounds = match reading {
            Reading::Histogram(buckets) => buckets.iter().map(|(bound, _)| *bound).collect(),
            Reading::Scalar(_) => Arc::from([]),
        };
        self.bounds.push(bounds);
        id
    }

    fn reading(&self, id: usize, kept: &Kept) -> Option<Reading> {
        match kept {
            Kept::Missing => None,
            Kept::Scalar(value) => Some(Reading::Scalar(*value)),
            Kept::Histogram(counts) => {
                let bounds = self.bounds.get(id)?;
                Some(Reading::Histogram(
                    bounds.iter().copied().zip(counts.iter().copied()).collect(),
                ))
            }
        }
    }
}

/// Every series of the metrics, now
pub fn sample() -> Sample {
    let mut series = vec![];
    for family in super::families() {
        for metric in family.get_metric() {
            let mut labels = metric
                .get_label()
                .iter()
                .map(|label| (label.name().to_string(), label.value().to_string()))
                .collect::<Vec<_>>();
            labels.sort();

            let reading = match family.get_field_type() {
                prometheus::proto::MetricType::COUNTER => {
                    Reading::Scalar(metric.get_counter().value())
                }
                prometheus::proto::MetricType::GAUGE => {
                    Reading::Scalar(metric.get_gauge().value())
                }
                prometheus::proto::MetricType::HISTOGRAM => {
                    let histogram = metric.get_histogram();
                    let mut buckets = histogram
                        .get_bucket()
                        .iter()
                        .map(|bucket| (bucket.upper_bound(), bucket.cumulative_count()))
                        .collect::<Vec<_>>();
                    // `+Inf` isn't listed: it's every observation
                    buckets.push((f64::INFINITY, histogram.get_sample_count()));
                    Reading::Histogram(buckets)
                }
                _ => continue,
            };
            series.push(((family.name().to_string(), labels), reading));
        }
    }
    Sample::new(Instant::now(), &series, super::process::read())
}

/// Keeps a sample of the metrics, thinning out older ones
pub fn record() {
    let sample = Arc::new(sample());
    let mut history = HISTORY.lock().unwrap_or_else(PoisonError::into_inner);
    history.push_back(sample);
    thin(&mut history);
}

static HISTORY: Mutex<VecDeque<Arc<Sample>>> = Mutex::new(VecDeque::new());

/// When the stats were last read
static LAST_READ: Mutex<Option<Instant>> = Mutex::new(None);

/// Whether the stats were read in the last 5 minutes
fn read_lately() -> bool {
    LAST_READ
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .is_some_and(|read| read.elapsed() <= OLDEST)
}

/// Every sample of the last minute, then one every 10 seconds, up to 5 minutes
fn thin(history: &mut VecDeque<Arc<Sample>>) {
    let Some(latest) = history.back().map(|sample| sample.at) else {
        return;
    };
    let mut previous: Option<Instant> = None;
    history.retain(|sample| {
        let age = latest.saturating_duration_since(sample.at);
        let keep = age <= DENSE
            || (age <= OLDEST
                && previous.is_none_or(|previous| {
                    sample.at.saturating_duration_since(previous) >= SPARSE
                }));
        if keep && age > DENSE {
            previous = Some(sample.at);
        }
        keep
    });
}

/// Samples the metrics every second while the stats are read; drops the samples otherwise
pub async fn run() {
    let mut interval = tokio::time::interval(Duration::from_secs(1));
    loop {
        interval.tick().await;
        if read_lately() {
            record();
        } else {
            HISTORY.lock().unwrap_or_else(PoisonError::into_inner).clear();
        }
    }
}

/// The sample about `window` before the latest one (or the oldest, if there's none that old
/// yet), and the latest one
pub fn window(window: Duration) -> Option<(Arc<Sample>, Arc<Sample>)> {
    *LAST_READ.lock().unwrap_or_else(PoisonError::into_inner) = Some(Instant::now());
    let mut history = HISTORY.lock().unwrap_or_else(PoisonError::into_inner);
    // Not sampled lately: from now on
    if history.is_empty() {
        history.push_back(Arc::new(sample()));
    }
    pick(&history, window)
}

fn pick(samples: &VecDeque<Arc<Sample>>, window: Duration) -> Option<(Arc<Sample>, Arc<Sample>)> {
    let latest = samples.back()?;
    let earlier = samples
        .iter()
        .rev()
        .find(|sample| latest.at.saturating_duration_since(sample.at) >= window)
        .or(samples.front())?;
    Some((earlier.clone(), latest.clone()))
}

/// The `q` quantile (0 to 1) of histogram buckets (cumulative counts by upper bound),
/// interpolated within its bucket as Prometheus' `histogram_quantile` does
pub fn quantile(buckets: &[(f64, u64)], q: f64) -> Option<f64> {
    let total = buckets.last()?.1;
    if total == 0 {
        return None;
    }
    #[allow(clippy::cast_precision_loss)]
    let rank = q * total as f64;

    let mut lower = (0.0, 0);
    for &(upper, count) in buckets {
        #[allow(clippy::cast_precision_loss)]
        if count as f64 >= rank {
            // Above the last finite bound, the last finite bound is all we know
            if upper.is_infinite() {
                return Some(lower.0);
            }
            let in_bucket = (count - lower.1) as f64;
            if in_bucket == 0.0 {
                return Some(upper);
            }
            return Some(lower.0 + (upper - lower.0) * (rank - lower.1 as f64) / in_bucket);
        }
        lower = (upper, count);
    }
    Some(lower.0)
}

/// Stats between `earlier` and `latest`, grouped by host, location and upstream, and by `tcp`
/// block
pub fn stats(earlier: &Sample, latest: &Sample) -> Value {
    let window = latest.at.saturating_duration_since(earlier.at).as_secs_f64();
    let (hosts, tcp) = collect(earlier, latest);
    let mut stats = render(window, hosts, tcp);
    if let Some(stats) = stats.as_object_mut() {
        stats.insert(
            "process".to_string(),
            process(window, &earlier.process, &latest.process),
        );
    }
    stats
}

/// CPU used over the window (% of one core), memory now, and the container's limits
fn process(
    window: f64,
    earlier: &super::process::Reading,
    latest: &super::process::Reading,
) -> Value {
    let cpu = (latest.cpu_seconds - earlier.cpu_seconds).max(0.0);
    let limits = super::process::limits();
    json!({
        "cpu_percent": if window > 0.0 { round(cpu / window * 100.0) } else { 0.0 },
        "cpu_seconds": round(latest.cpu_seconds),
        "memory_bytes": latest.rss_bytes,
        "allocator_bytes": latest.allocator_bytes,
        "memory_limit_bytes": limits.memory_bytes,
        "cpu_limit": limits.cpus,
    })
}

type Hosts = BTreeMap<String, BTreeMap<String, Location>>;

/// What changed between the samples, by host and location, and by `tcp` block
fn collect(earlier: &Sample, latest: &Sample) -> (Hosts, BTreeMap<String, Tcp>) {
    let mut hosts = Hosts::new();
    let mut tcp: BTreeMap<String, Tcp> = BTreeMap::new();
    let known = SERIES.lock().unwrap_or_else(PoisonError::into_inner);
    let Some(known) = known.as_ref() else {
        return (hosts, tcp);
    };

    for (id, kept) in latest.readings.iter().enumerate() {
        let (Some((name, labels)), Some(reading)) = (known.keys.get(id), known.reading(id, kept))
        else {
            continue;
        };
        let reading = &reading;
        let before = earlier
            .readings
            .get(id)
            .and_then(|kept| known.reading(id, kept));
        let label = |label: &str| {
            labels
                .iter()
                .find(|(name, _)| name == label)
                .map_or("", |(_, value)| value.as_str())
        };
        let delta = || match (reading, before.as_ref()) {
            (Reading::Scalar(latest), Some(Reading::Scalar(earlier))) => {
                (latest - earlier).max(0.0)
            }
            (Reading::Scalar(latest), _) => *latest,
            (Reading::Histogram(_), _) => 0.0,
        };
        let histogram = || match (reading, before.as_ref()) {
            (Reading::Histogram(latest), Some(Reading::Histogram(earlier))) => latest
                .iter()
                .zip(earlier)
                .map(|((bound, latest), (_, earlier))| (*bound, latest.saturating_sub(*earlier)))
                .collect(),
            (Reading::Histogram(latest), _) => latest.clone(),
            (Reading::Scalar(_), _) => vec![],
        };
        let (host, path) = (label("host"), label("location"));

        match name.as_str() {
            "proksi_requests_total" => {
                let location = location_mut(&mut hosts, host, path);
                let delta = delta();
                location.requests += delta;
                if delta > 0.0 {
                    location.responses.insert(label("status").to_string(), delta);
                }
            }
            "proksi_request_duration_seconds" => {
                location_mut(&mut hosts, host, path).duration = histogram();
            }
            "proksi_blocked_requests_total" => {
                let delta = delta();
                if delta > 0.0 {
                    location_mut(&mut hosts, host, path)
                        .blocked
                        .insert(label("block").to_string(), delta);
                }
            }
            "proksi_upstream_duration_seconds" => {
                upstream_mut(&mut hosts, host, path, label("upstream")).duration = histogram();
            }
            "proksi_upstream_errors_total" => {
                upstream_mut(&mut hosts, host, path, label("upstream")).errors = delta();
            }
            "proksi_upstream_healthy" => {
                upstream_mut(&mut hosts, host, path, label("upstream")).healthy =
                    matches!(reading, Reading::Scalar(value) if *value > 0.0);
            }
            "proksi_tcp_connections_total" => {
                tcp.entry(label("block").to_string()).or_default().connections = delta();
            }
            "proksi_tcp_connections_active" => {
                if let Reading::Scalar(active) = reading {
                    tcp.entry(label("block").to_string()).or_default().active = *active;
                }
            }
            "proksi_tcp_bytes_total" => {
                let block = tcp.entry(label("block").to_string()).or_default();
                if label("direction") == "in" {
                    block.bytes_in = delta();
                } else {
                    block.bytes_out = delta();
                }
            }
            "proksi_tcp_upstream_errors_total" => {
                tcp.entry(label("block").to_string()).or_default().upstream_errors = delta();
            }
            "proksi_tcp_connection_duration_seconds" => {
                tcp.entry(label("block").to_string()).or_default().duration = histogram();
            }
            _ => {}
        }
    }
    (hosts, tcp)
}

fn render(window: f64, hosts: Hosts, tcp: BTreeMap<String, Tcp>) -> Value {
    let per_second = |value: f64| {
        if window > 0.0 {
            round(value / window)
        } else {
            0.0
        }
    };

    // Every host and location together
    let mut total = Location::default();
    for location in hosts.values().flat_map(BTreeMap::values) {
        total.requests += location.requests;
        for (status, value) in &location.responses {
            *total.responses.entry(status.clone()).or_default() += value;
        }
        add_buckets(&mut total.duration, &location.duration);
    }
    let total = json!({
        "requests_per_second": per_second(total.requests),
        "responses": counts(&total.responses),
        "duration_ms": percentiles(&total.duration),
    });

    let hosts = hosts
        .into_iter()
        .map(|(host, locations)| {
            let locations = locations
                .into_iter()
                .map(|(path, location)| {
                    let upstreams = location
                        .upstreams
                        .iter()
                        .map(|(upstream, stats)| {
                            // A histogram's last bucket (`+Inf`) counts every request
                            #[allow(clippy::cast_precision_loss)]
                            let requests =
                                stats.duration.last().map_or(0.0, |(_, count)| *count as f64);
                            json!({
                                "upstream": upstream,
                                "requests_per_second": per_second(requests),
                                "healthy": stats.healthy,
                                "errors": count(stats.errors),
                                "duration_ms": percentiles(&stats.duration),
                            })
                        })
                        .collect::<Vec<_>>();
                    json!({
                        "location": path,
                        "requests_per_second": per_second(location.requests),
                        "responses": counts(&location.responses),
                        "duration_ms": percentiles(&location.duration),
                        "blocked": counts(&location.blocked),
                        "upstreams": upstreams,
                    })
                })
                .collect::<Vec<_>>();
            json!({ "host": host, "locations": locations })
        })
        .collect::<Vec<_>>();

    let tcp = tcp
        .into_iter()
        .map(|(block, stats)| {
            json!({
                "block": block,
                "connections_per_second": per_second(stats.connections),
                "active": count(stats.active),
                "bytes_in_per_second": per_second(stats.bytes_in),
                "bytes_out_per_second": per_second(stats.bytes_out),
                "upstream_errors": count(stats.upstream_errors),
                "duration_ms": percentiles(&stats.duration),
            })
        })
        .collect::<Vec<_>>();

    json!({ "window_seconds": round(window), "total": total, "hosts": hosts, "tcp": tcp })
}

/// Adds histogram buckets to `sum` (every histogram of a metric has the same bounds)
fn add_buckets(sum: &mut Vec<(f64, u64)>, buckets: &[(f64, u64)]) {
    if sum.is_empty() {
        sum.extend_from_slice(buckets);
        return;
    }
    for ((bound, count), (other, add)) in sum.iter_mut().zip(buckets) {
        // The same bounds, from the same metric
        if bound.to_bits() == other.to_bits() {
            *count += add;
        }
    }
}

#[derive(Default)]
struct Location {
    requests: f64,
    responses: BTreeMap<String, f64>,
    duration: Vec<(f64, u64)>,
    blocked: BTreeMap<String, f64>,
    upstreams: BTreeMap<String, Upstream>,
}

#[derive(Default)]
struct Upstream {
    duration: Vec<(f64, u64)>,
    errors: f64,
    healthy: bool,
}

#[derive(Default)]
struct Tcp {
    connections: f64,
    active: f64,
    bytes_in: f64,
    bytes_out: f64,
    upstream_errors: f64,
    duration: Vec<(f64, u64)>,
}

fn location_mut<'a>(
    hosts: &'a mut Hosts,
    host: &str,
    location: &str,
) -> &'a mut Location {
    hosts
        .entry(host.to_string())
        .or_default()
        .entry(location.to_string())
        .or_default()
}

fn upstream_mut<'a>(
    hosts: &'a mut Hosts,
    host: &str,
    location: &str,
    upstream: &str,
) -> &'a mut Upstream {
    location_mut(hosts, host, location)
        .upstreams
        .entry(upstream.to_string())
        .or_default()
}

/// p50, p90 and p99 in milliseconds (`null` without observations)
fn percentiles(buckets: &[(f64, u64)]) -> Value {
    let ms = |q| quantile(buckets, q).map(|seconds| round(seconds * 1000.0));
    json!({ "p50": ms(0.5), "p90": ms(0.9), "p99": ms(0.99) })
}

fn counts(values: &BTreeMap<String, f64>) -> Value {
    Value::Object(
        values
            .iter()
            .map(|(name, value)| (name.clone(), json!(count(*value))))
            .collect::<Map<_, _>>(),
    )
}

#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
fn count(value: f64) -> u64 {
    value.max(0.0).round() as u64
}

/// Rounded to 3 decimals
fn round(value: f64) -> f64 {
    (value * 1000.0).round() / 1000.0
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use serde_json::json;

    use super::{quantile, stats, Key, Reading, Sample};

    #[test]
    fn test_pick_a_window() {
        let start = Instant::now();
        let samples = (0..=10)
            .map(|second| std::sync::Arc::new(sample(start + Duration::from_secs(second), &[])))
            .collect::<std::collections::VecDeque<_>>();
        let seconds = |(earlier, latest): (std::sync::Arc<Sample>, std::sync::Arc<Sample>)| {
            (
                earlier.at.duration_since(start).as_secs(),
                latest.at.duration_since(start).as_secs(),
            )
        };

        assert_eq!(super::pick(&samples, Duration::from_secs(5)).map(seconds), Some((5, 10)));
        assert_eq!(super::pick(&samples, Duration::from_secs(1)).map(seconds), Some((9, 10)));
        // Not that old yet: since the oldest
        assert_eq!(super::pick(&samples, Duration::from_secs(60)).map(seconds), Some((0, 10)));
        assert!(super::pick(&std::collections::VecDeque::new(), Duration::from_secs(5)).is_none());
    }

    #[test]
    fn test_older_samples_are_thinned() {
        let start = Instant::now();
        let mut history = std::collections::VecDeque::new();
        for second in 0..=400 {
            history.push_back(std::sync::Arc::new(sample(start + Duration::from_secs(second), &[])));
            super::thin(&mut history);
        }

        let ages = history
            .iter()
            .map(|sample| (start + Duration::from_secs(400)).duration_since(sample.at).as_secs())
            .collect::<Vec<_>>();
        // Every second of the last minute, then one every 10 seconds, up to 5 minutes
        assert!(ages.len() < 90, "{ages:?}");
        assert!((0..=60).all(|age| ages.contains(&age)), "{ages:?}");
        assert!(ages.iter().all(|age| *age <= 301), "{ages:?}");
        assert!(*ages.first().unwrap() >= 290, "{ages:?}");
        // A 5-minute window still has a sample about 5 minutes old
        let (earlier, latest) = super::pick(&history, Duration::from_secs(300)).unwrap();
        assert!(latest.at.duration_since(earlier.at).as_secs() >= 290);
    }

    #[test]
    fn test_sample_and_record() {
        crate::metrics::LocationMetrics::new("live-sample.test", "/", &[])
            .record(200, Duration::from_millis(3));

        let sample = super::sample();
        assert_eq!(
            sample.get(&key(
                "proksi_requests_total",
                &[("host", "live-sample.test"), ("location", "/"), ("status", "2xx")]
            )),
            Some(Reading::Scalar(1.0))
        );
        let Some(Reading::Histogram(buckets)) = sample.get(&key(
            "proksi_request_duration_seconds",
            &[("host", "live-sample.test"), ("location", "/")],
        )) else {
            panic!("no histogram");
        };
        assert_eq!(buckets.last(), Some(&(f64::INFINITY, 1)));

        // Reading the stats starts sampling, from that moment
        assert!(super::window(Duration::from_secs(60)).is_some());
        assert!(super::read_lately());
    }

    #[test]
    fn test_quantile() {
        // 10 below 1, 10 between 1 and 2, nothing above
        let buckets = [(1.0, 10), (2.0, 20), (f64::INFINITY, 20)];

        assert_eq!(quantile(&buckets, 0.5), Some(1.0));
        assert_eq!(quantile(&buckets, 0.25), Some(0.5));
        assert_eq!(quantile(&buckets, 0.75), Some(1.5));
        assert_eq!(quantile(&buckets, 1.0), Some(2.0));
        // Nothing observed
        assert_eq!(quantile(&[(1.0, 0), (f64::INFINITY, 0)], 0.5), None);
        // Above the last bound: the last bound
        assert_eq!(quantile(&[(1.0, 0), (f64::INFINITY, 4)], 0.5), Some(1.0));
    }

    fn key(name: &str, labels: &[(&str, &str)]) -> Key {
        let mut labels = labels
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect::<Vec<_>>();
        labels.sort();
        (name.to_string(), labels)
    }

    fn sample(at: Instant, series: &[(Key, Reading)]) -> Sample {
        let process = crate::metrics::process::Reading {
            cpu_seconds: 0.0,
            rss_bytes: 2048,
            allocator_bytes: 1024,
        };
        Sample::new(at, series, process)
    }

    fn histogram(below_10ms: u64, below_100ms: u64) -> Reading {
        Reading::Histogram(vec![
            (0.01, below_10ms),
            (0.1, below_100ms),
            (f64::INFINITY, below_100ms),
        ])
    }

    #[test]
    fn test_stats() {
        let location = [("host", "a.test"), ("location", "/")];
        let upstream = [("host", "a.test"), ("location", "/"), ("upstream", "10.0.0.1:80")];
        let requests = |status: &str| {
            key(
                "proksi_requests_total",
                &[location[0], location[1], ("status", status)],
            )
        };
        let tcp = [("block", "tcp \"db\"")];
        let start = Instant::now();

        let earlier = sample(
            start,
            &[
                (requests("2xx"), Reading::Scalar(100.0)),
                (requests("5xx"), Reading::Scalar(0.0)),
                (key("proksi_request_duration_seconds", &location), histogram(100, 100)),
                (key("proksi_upstream_duration_seconds", &upstream), histogram(100, 100)),
                (key("proksi_upstream_errors_total", &upstream), Reading::Scalar(0.0)),
                (key("proksi_tcp_connections_total", &tcp), Reading::Scalar(1.0)),
                (
                    key("proksi_tcp_bytes_total", &[tcp[0], ("direction", "in")]),
                    Reading::Scalar(0.0),
                ),
            ],
        );
        let mut latest = sample(
            start + Duration::from_secs(10),
            &[
                // 20 requests in 10 seconds: 18 fast, 2 slow; 2 errors
                (requests("2xx"), Reading::Scalar(118.0)),
                (requests("5xx"), Reading::Scalar(2.0)),
                (key("proksi_request_duration_seconds", &location), histogram(118, 120)),
                (key("proksi_upstream_duration_seconds", &upstream), histogram(118, 120)),
                (key("proksi_upstream_errors_total", &upstream), Reading::Scalar(2.0)),
                (key("proksi_upstream_healthy", &upstream), Reading::Scalar(1.0)),
                (
                    key(
                        "proksi_blocked_requests_total",
                        &[location[0], location[1], ("block", "rate_limit")],
                    ),
                    Reading::Scalar(3.0),
                ),
                (key("proksi_tcp_connections_total", &tcp), Reading::Scalar(11.0)),
                (key("proksi_tcp_connections_active", &tcp), Reading::Scalar(4.0)),
                (key("proksi_tcp_upstream_errors_total", &tcp), Reading::Scalar(3.0)),
                (
                    key("proksi_tcp_bytes_total", &[tcp[0], ("direction", "in")]),
                    Reading::Scalar(5000.0),
                ),
            ],
        );

        latest.process.cpu_seconds = 1.5;

        let stats = stats(&earlier, &latest);

        assert_eq!(stats["window_seconds"], json!(10.0));
        let location = &stats["hosts"][0]["locations"][0];
        assert_eq!(stats["hosts"][0]["host"], "a.test");
        assert_eq!(location["location"], "/");
        assert_eq!(location["requests_per_second"], json!(2.0));
        assert_eq!(location["responses"], json!({ "2xx": 18, "5xx": 2 }));
        // 18 of 20 under 10 ms: p50 within the first bucket, p99 within the second
        assert_eq!(location["duration_ms"]["p50"], json!(5.556));
        assert_eq!(location["duration_ms"]["p99"], json!(91.0));
        // Requests answered by a block in the window
        assert_eq!(location["blocked"], json!({ "rate_limit": 3 }));

        let upstream = &location["upstreams"][0];
        assert_eq!(upstream["upstream"], "10.0.0.1:80");
        assert_eq!(upstream["healthy"], json!(true));
        assert_eq!(upstream["errors"], json!(2));
        // Requests sent to it: its histogram's count
        assert_eq!(upstream["requests_per_second"], json!(2.0));
        assert_eq!(upstream["duration_ms"]["p50"], json!(5.556));

        let tcp = &stats["tcp"][0];
        assert_eq!(tcp["block"], "tcp \"db\"");
        assert_eq!(tcp["connections_per_second"], json!(1.0));
        assert_eq!(tcp["active"], json!(4));
        assert_eq!(tcp["bytes_in_per_second"], json!(500.0));
        assert_eq!(tcp["bytes_out_per_second"], json!(0.0));
        // Connections that couldn't reach an upstream in the window
        assert_eq!(tcp["upstream_errors"], json!(3));

        // Every host and location together
        assert_eq!(stats["total"]["requests_per_second"], json!(2.0));
        assert_eq!(stats["total"]["responses"], json!({ "2xx": 18, "5xx": 2 }));
        assert_eq!(stats["total"]["duration_ms"]["p50"], json!(5.556));
        // 1.5 seconds of CPU in 10 seconds
        assert_eq!(stats["process"]["cpu_percent"], json!(15.0));
        assert_eq!(stats["process"]["memory_bytes"], json!(2048));
        assert_eq!(stats["process"]["allocator_bytes"], json!(1024));
    }

    #[test]
    fn test_totals_add_up_locations() {
        let start = Instant::now();
        let requests = |location| {
            key(
                "proksi_requests_total",
                &[("host", "a.test"), ("location", location), ("status", "2xx")],
            )
        };
        let duration = |location| {
            key(
                "proksi_request_duration_seconds",
                &[("host", "a.test"), ("location", location)],
            )
        };
        let earlier = sample(start, &[]);
        let latest = sample(
            start + Duration::from_secs(1),
            &[
                (requests("/a"), Reading::Scalar(3.0)),
                (requests("/b"), Reading::Scalar(1.0)),
                (duration("/a"), histogram(3, 3)),
                (duration("/b"), histogram(0, 1)),
            ],
        );

        let stats = stats(&earlier, &latest);

        assert_eq!(stats["total"]["requests_per_second"], json!(4.0));
        assert_eq!(stats["total"]["responses"], json!({ "2xx": 4 }));
        // 3 fast and 1 slow: the 90th percentile is in the slow bucket
        assert!(stats["total"]["duration_ms"]["p90"].as_f64().unwrap() > 10.0, "{stats}");
    }

    #[test]
    fn test_stats_without_requests() {
        let start = Instant::now();
        let location = [("host", "a.test"), ("location", "/")];
        let quiet = sample(
            start,
            &[(key("proksi_request_duration_seconds", &location), histogram(0, 0))],
        );

        let stats = stats(&quiet, &quiet);

        let location = &stats["hosts"][0]["locations"][0];
        assert_eq!(location["requests_per_second"], json!(0.0));
        assert_eq!(location["duration_ms"]["p50"], json!(null));
    }
}
