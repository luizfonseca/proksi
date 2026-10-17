# Migrating from nginx

Proksi's configuration is close to nginx's: `host` blocks are `server` blocks, and `location` blocks are `location` blocks. This page covers the differences, for a configuration file and for Kubernetes (ingress-nginx).

## Concepts

| nginx | Proksi |
| --- | --- |
| `server { server_name ... }` | `host "<name>" { }` (one name per host) |
| `listen 443 ssl` | `server.https_address`, or a host's `listen` |
| `location /api { }` | `location "/api" { }` |
| `upstream app { server ...; }` and `proxy_pass` | `upstream "<host:port>" { }` blocks in the location |
| `ssl_certificate`, `ssl_certificate_key` | `tls { certificate { pem, key } }`, or `lets_encrypt` |
| `stream { }` | `tcp` blocks |
| `nginx -s reload` | [Auto reload](configuration/auto-reload.md), or `SIGHUP` (see [Restarts and upgrades](configuration/upgrades.md)) |

## A host

```nginx
upstream app {
  server 10.0.0.1:8080 weight=3;
  server 10.0.0.2:8080;
}

server {
  listen 80;
  server_name app.example.com;
  return 301 https://$host$request_uri;
}

server {
  listen 443 ssl;
  http2 on;
  server_name app.example.com;
  ssl_certificate     /etc/letsencrypt/live/app.example.com/fullchain.pem;
  ssl_certificate_key /etc/letsencrypt/live/app.example.com/privkey.pem;
  client_max_body_size 10m;

  location / {
    proxy_pass http://app;
    proxy_set_header Host $host;
    proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;
    proxy_set_header X-Forwarded-Proto $scheme;
    proxy_http_version 1.1;
    proxy_set_header Upgrade $http_upgrade;
    proxy_set_header Connection "upgrade";
  }
}
```

```hcl
host "app.example.com" {
  tls {
    certificate {
      pem = "/etc/letsencrypt/live/app.example.com/fullchain.pem"
      key = "/etc/letsencrypt/live/app.example.com/privkey.pem"
    }
  }

  location "/" {
    upstream "10.0.0.1:8080" { weight = 3 }
    upstream "10.0.0.2:8080" {}

    body_limit {
      max = "10MiB"
    }
  }
}
```

What you no longer write:

- **The redirect to HTTPS**: hosts with TLS redirect HTTP requests (`308`).
- **HTTP/2** is on for HTTPS.
- **`Host`**: the client's `Host` is forwarded as it is. (nginx sends the upstream's name unless `proxy_set_header Host $host`.) To send another one, set it on the upstream: `headers { add = { "Host" = "..." } }`.
- **Forwarded headers**: `X-Forwarded-For`, `X-Forwarded-Proto`, `X-Forwarded-Host` and `X-Real-IP` are set. Behind another proxy, list it in `server.trusted_proxies` (as `set_real_ip_from`).
- **WebSockets** are proxied without `Upgrade`/`Connection` headers.
- **Certificates from certbot** can stay: the files are checked every 10 seconds, so a renewal is served without a reload. Or let Proksi order them with `lets_encrypt`, and drop certbot.

Mind the defaults that differ:

- **Body size**: nginx refuses bodies over 1 MB unless `client_max_body_size` says otherwise. Proksi has no limit unless a location has a `body_limit`.
- **Locations** match their path and everything below it: `location "/api"` serves `/api` and `/api/users`, but not `/apis` (nginx's `location /api` serves `/apis` too). The most specific location wins. The `=`, `~`, `~*` and `^~` modifiers and regular expressions aren't supported.
- **`proxy_pass` with a path** (`proxy_pass http://app/v2/;`) replaces the matched prefix; Proksi always forwards the path unchanged.

## Directives

| nginx | Proksi |
| --- | --- |
| `limit_req_zone` / `limit_req` | [`rate_limit { requests, per }`](plugins/rate-limit.md), per client |
| `client_max_body_size` | [`body_limit { max }`](plugins/body-limit.md) |
| `auth_basic` | [`basic_auth { user, pass }`](plugins/basic-auth.md) (one user, no htpasswd file) |
| `add_header` | `headers { response { add = { ... } } }` |
| `proxy_hide_header` | `headers { response { remove = [...] } }` |
| `proxy_set_header` | `headers { add = { ... } }` in an `upstream` |
| `proxy_cache` | [`cache { }`](use-cases/cache.md) |
| `keepalive_timeout`, `client_header_timeout` | `server.timeouts.idle_secs` |
| `client_body_timeout` | `server.timeouts.read_secs`, or per host/location |
| `send_timeout` | `server.timeouts.write_secs`, or per host/location |
| `access_log` | JSON access logs on stdout (see [Logging](configuration/logging.md)) |
| `stub_status` | The [admin API](configuration/admin.md): Prometheus metrics and live stats |
| `max_fails`, `fail_timeout` | Health checks: a TCP connection to each upstream every 15 seconds |
| `least_conn`, `ip_hash`, `hash` | Not available (weighted round robin only) |
| `root`, `try_files`, `return`, `rewrite`, `gzip`, `allow`/`deny` | Not available: Proksi only proxies |

## TCP (`stream`)

```nginx
stream {
  server {
    listen 5433 ssl;
    ssl_certificate     /certs/db.pem;
    ssl_certificate_key /certs/db.key;
    proxy_pass 10.0.0.5:5432;
  }
}
```

```hcl
host "db.example.com" {
  tls {
    certificate {
      pem = "/certs/db.pem"
      key = "/certs/db.key"
    }
  }

  tcp "postgres" {
    listen = "0.0.0.0:5433"
    upstream "10.0.0.5:5432" {}
  }
}
```

A `tcp` block at the top level forwards plain TCP; `tls = "passthrough"` forwards TLS untouched, chosen by name (as `ssl_preread`). See [TCP](routing/tcp.md).

## Kubernetes (ingress-nginx)

ingress-nginx has been retired by the Kubernetes project. Proksi serves the same `Ingress` resources (see [Kubernetes](installation/kubernetes.md)); set `ingressClassName: proksi`, and replace the `nginx.ingress.kubernetes.io/*` annotations:

| ingress-nginx annotation | Proksi |
| --- | --- |
| `ssl-redirect`, `force-ssl-redirect` | Built in; `proksi.ingress/tls: "false"` for plain HTTP |
| `proxy-body-size` | `body_limit { max = "..." }` in `proksi.ingress/blocks` |
| `limit-rps`, `limit-rpm` | `rate_limit { ... }` in `proksi.ingress/blocks` |
| `auth-type: basic`, `auth-secret` | `basic_auth {}` in `proksi.ingress/blocks`, and `proksi.ingress/auth-secret`, naming a `Secret` labelled `proksi.ingress/auth` with `user` and `pass` keys (not an htpasswd file) |
| `configuration-snippet` (`more_set_headers`) | `headers { response { add = { ... } } }` in `proksi.ingress/blocks` |
| `proxy-read-timeout`, `proxy-send-timeout` | `timeouts { read_secs, write_secs }` in `proksi.ingress/blocks` |
| `service-upstream` | `proksi.ingress/upstream: service` |
| `rewrite-target`, `backend-protocol`, `affinity`, `whitelist-source-range` | Not available |

cert-manager works as before, including HTTP-01 challenges (set the solver's class to `proksi`, see [cert-manager](installation/kubernetes.md#cert-manager)).
