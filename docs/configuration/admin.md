# Admin API and metrics

Proksi can serve metrics and an admin API on an address of its own. It's off unless the configuration has an `admin` block:

```hcl
admin {
  listen = "127.0.0.1:7555"
  token  = env("PROKSI_ADMIN_TOKEN")
}
```

Every request needs the token, as `Authorization: Bearer <token>`. Without a `token`, `listen` must be a loopback address (`127.0.0.1`, `::1`), so the admin API is never reachable from another machine without one.

The admin listener is plain HTTP. To reach it from outside, route a host to it and put `basic_auth`, `oauth2` or another plugin in front:

```hcl
host "admin.example.com" {
  location "/" {
    basic_auth {
      user = "admin"
      pass = env("ADMIN_PASSWORD")
    }
    upstream "127.0.0.1:7555" {}
  }
}
```

Like the other listeners, the `admin` block is read when Proksi starts: changing it restarts Proksi.

## Metrics

`GET /metrics` returns the metrics in the Prometheus text format:

| Metric | Labels | |
| --- | --- | --- |
| `proksi_requests_total` | `host`, `location`, `status` | Requests, by status class (`1xx`…`5xx`, or `none` when no response was sent) |
| `proksi_request_duration_seconds` | `host`, `location` | Time to answer requests (histogram) |
| `proksi_upstream_duration_seconds` | `host`, `location`, `upstream` | Time until the upstream's response headers (histogram) |
| `proksi_upstream_errors_total` | `host`, `location`, `upstream` | Failures to connect to an upstream, or while proxying to it |
| `proksi_upstream_healthy` | `host`, `location`, `upstream` | `1` while the health checks consider the upstream healthy, `0` otherwise |
| `proksi_blocked_requests_total` | `host`, `location`, `block` | Requests answered by a plugin instead of the upstream (`rate_limit`, `body_limit`, `basic_auth`, `oauth2`) |
| `proksi_tcp_connections_total` | `block` | Connections accepted by a `tcp` block (`block` is how it's named in logs: `tcp "db" in host "a.com"`) |
| `proksi_tcp_connections_active` | `block` | Connections open right now |
| `proksi_tcp_bytes_total` | `block`, `direction` | Bytes from the client (`in`) and to it (`out`), counted while the connection is open |
| `proksi_tcp_connection_duration_seconds` | `block` | How long connections stay open (histogram) |
| `proksi_certificate_expiry_timestamp_seconds` | `domain` | When each certificate expires (refreshed every minute) |
| `proksi_routes` | `source` | Hosts applied from each source (`file`, `docker`, `kubernetes`, `store`) |

Labels come from the configuration: `host` is the name of the host that served the request (`*` for the catch-all), `location` its path, and `upstream` its address as configured. Requests no location served (unknown hosts, redirects to HTTPS, ACME challenges) are counted under `host="_unknown"`, so clients can't create new series by sending new names or paths.

Histograms go from 0.5 ms to 10 s. With Prometheus, the 99th percentile of a host's requests over the last 5 minutes is:

```promql
histogram_quantile(0.99, sum by (le) (rate(proksi_request_duration_seconds_bucket{host="example.com"}[5m])))
```

Certificates expiring in less than 2 weeks:

```promql
proksi_certificate_expiry_timestamp_seconds - time() < 14 * 86400
```

A scrape configuration with the token:

```yaml
scrape_configs:
  - job_name: proksi
    authorization:
      credentials: "<token>"
    static_configs:
      - targets: ["127.0.0.1:7555"]
```

Recording metrics doesn't add lookups to the request path: a location's metrics are created when it's built, and each request only increments counters it already holds.

See [ADR 0004](adrs/0004-admin-api-and-metrics.md) for what comes next: live stats, status, certificates and routes.
