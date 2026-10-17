# Migrating from Caddy

Like Caddy, Proksi gets and renews certificates for its hosts, and redirects HTTP to HTTPS without being told to. This page maps a Caddyfile, caddy-docker-proxy labels and the layer4 plugin to Proksi.

## Concepts

| Caddy | Proksi |
| --- | --- |
| Site block (`app.example.com { }`) | `host "app.example.com" { }` (one name per host) |
| `reverse_proxy` | `upstream` blocks of a `location` |
| `handle /api/*` | `location "/api"` (the path and everything below it) |
| Automatic HTTPS | `lets_encrypt { enabled = true }`, for every host with TLS |
| `tls internal` (local CA) | `tls { self_signed_on_failure = true }` |
| `tls cert.pem key.pem` | `tls { certificate { pem, key } }` |
| Global options (`email`, `acme_ca`) | `lets_encrypt { email, staging }` |
| `caddy reload` | [Auto reload](configuration/auto-reload.md), or `SIGHUP` (see [Restarts and upgrades](configuration/upgrades.md)) |
| Admin API (`:2019`) | The [admin API](configuration/admin.md): status, metrics, live stats, and routes written to the store |

## A site

```caddyfile
{
  email you@example.com
}

app.example.com {
  encode gzip
  header -Server

  handle /api/* {
    reverse_proxy api-1:8080 api-2:8080
  }

  handle {
    reverse_proxy web:3000
  }
}
```

```hcl
lets_encrypt {
  enabled = true
  staging = false
  email   = "you@example.com"
}

host "app.example.com" {
  headers {
    response {
      remove = ["Server"]
    }
  }

  location "/api" {
    upstream "api-1:8080" {}
    upstream "api-2:8080" {}
  }

  location "/" {
    upstream "web:3000" {}
  }
}
```

- Let's Encrypt is **off by default** (unlike Caddy), and uses its staging server unless `staging = false`. Certificates are kept in `paths.lets_encrypt` (a volume in containers), or in the [store](configuration/redis.md) when several Proksi instances run.
- As with Caddy, the client's `Host` is forwarded unchanged, and `X-Forwarded-For`, `X-Forwarded-Proto` and `X-Forwarded-Host` are set (plus `X-Real-IP`). `trusted_proxies` is `server.trusted_proxies`.
- WebSockets and HTTP/2 work without configuration.
- Upstream names (`web:3000`) are resolved again as their addresses change, like Caddy's dynamic upstreams.

## Directives

| Caddyfile | Proksi |
| --- | --- |
| `reverse_proxy a b` with `lb_policy weighted_round_robin` | Several `upstream` blocks, with `weight` |
| `health_uri`, `health_interval` | Health checks: a TCP connection to each upstream every 15 seconds (no HTTP path) |
| `header` (response) | `headers { response { add, remove } }` |
| `header_up` | `headers { add = { ... } }` in an `upstream` |
| `basic_auth` | [`basic_auth { user, pass }`](plugins/basic-auth.md) (one user, a plain password rather than a bcrypt hash) |
| `request_body { max_size }` | [`body_limit { max }`](plugins/body-limit.md) |
| `rate_limit` (plugin) | [`rate_limit { requests, per }`](plugins/rate-limit.md), built in |
| `cache` (plugin) | [`cache { }`](use-cases/cache.md), built in |
| `log` | JSON access logs on stdout (see [Logging](configuration/logging.md)) |
| Timeouts (`servers { timeouts { ... } }`) | `server.timeouts` (`idle_secs`, `read_secs`, `write_secs`) |
| `transport http { dial_timeout, read_timeout, write_timeout, keepalive }` | `timeouts { upstream { connect_secs, read_secs, write_secs, idle_secs } }`, in the server or per host/location |
| `grace_period` | `server.shutdown_timeout_secs` |
| `handle_path /api/*`, `uri strip_prefix`, `uri prefix` | `location "/api"` with [`rewrite`](routing/upstreams.md#rewriting-paths) `{ strip_prefix, add_prefix }` |
| `rewrite`, `uri path_regexp` | [`rewrite`](routing/upstreams.md#rewriting-paths) `{ match, replace }` |
| `redir` | Not available |
| `encode`, `file_server`, `php_fastcgi`, `templates`, `respond` | Not available: Proksi only proxies |
| On-demand TLS, wildcard sites (`*.example.com`), DNS challenges | Not available |

## Docker (caddy-docker-proxy)

```yaml
# caddy-docker-proxy
labels:
  caddy: app.example.com
  caddy.reverse_proxy: "{{upstreams 3000}}"
```

```yaml
# proksi (docker.enabled = true, and the container on a network proksi shares)
labels:
  proksi.enabled: "true"
  proksi.host: "app.example.com"
  proksi.port: "3000"
```

See [Docker](installation/docker.md) and [Docker swarm](use-cases/docker-swarm.md) for the other labels.

## TCP (layer4)

The layer4 plugin's routes become `tcp` blocks: at the top level (plain TCP, by port), or in a host (TLS terminated with the host's certificate, or passed through, chosen by name).

```hcl
host "db.example.com" {
  tcp "postgres" {
    listen = "0.0.0.0:5433"
    upstream "postgres:5432" {}
  }
}
```

See [TCP](routing/tcp.md).
