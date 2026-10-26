<p align="center">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="./docs/assets/logo-wordmark-dark.svg?v=1">
    <img src="./docs/assets/logo-wordmark-light.svg?v=1" alt="Proksi" width="260">
  </picture>
</p>

<hr/>

<picture>
  <source media="(prefers-color-scheme: dark)" srcset="./docs/assets/proksi-flow-dark.svg">
  <img src="./docs/assets/proksi-flow-light.svg" alt="proksi.hcl and a terminal on one side; on the other, Proksi's routes and events, live: a container's host appears with a certificate, an edit to proksi.hcl applies without dropping connections, and a rule rate limits a request">
</picture>


# Proksi

Proksi is a simple, lightweight, and easy-to-use proxy server that automatically handles SSL, HTTP, **TCP** and DNS traffic. It is designed to be used as a standalone proxy server or as a component in a larger system. Proksi is written in [Rust](https://www.rust-lang.org/) and uses [Pingora](https://github.com/cloudflare/pingora) as its core networking library.

<p>
    <a href="https://discord.gg/b22TRMew2m">
        <img src="https://github.com/luizfonseca/proksi/blob/main/assets/discord.png?raw=true" alt="discord-logo" width="100"/>
    </a>
</p>

## Contents

- [About](#about)
- [Features](#features)
- [Quick start](#quick-start)
  - [Configuration File Handling](#configuration-file-handling)
- [Configuration](#configuration)
  - [Server](#server)
  - [Hosts and locations](#hosts-and-locations)
    - [Ports, plain HTTP, and any host](#ports-plain-http-and-any-host)
  - [TCP](#tcp)
  - [Upstreams](#upstreams)
  - [Certificates](#certificates)
  - [Headers](#headers)
  - [Plugins](#plugins)
  - [Rules](#rules)
  - [Caching](#caching)
  - [Docker](#docker)
  - [Kubernetes](#kubernetes)
  - [Running many servers](#running-many-servers)
  - [Metrics](#metrics)
  - [Auto reload](#auto-reload)
  - [Functions and environment variables](#functions-and-environment-variables)
  - [YAML](#yaml)
  - [Migrating from `routes = [...]`](#migrating-from-routes--)
  - [What's next](#whats-next)
- [Documentation](#documentation)
- [Contributing](#contributing)
- [License](#license)


# Features

Of the many features Proksi offers is the ability to load balance to your infrastructure or any IP that supports your host configurations. Other features of Proksi also include:

- Automatic Docker and Docker Swarm service discovery through labels
- Built-in most common middlewares such as OAuth, JWT, Basic Auth, Rate Limiting, CDN Caching, static files and others
- Traffic splits (canaries, A/B tests), request mirroring, retries and least-connections load balancing
- The ability of running it as a single binary in your system
- Automatic SSL through Let's Encrypt (wildcard certificates with DNS challenges) and redirection from HTTP to HTTPS
- Hosts with their own ports, plain HTTP hosts (e.g. behind another load balancer) and a catch-all host
- Running many servers with the same routes and certificates (Redis, SQLite, Postgres)
- TCP proxying for anything that isn't HTTP, with or without TLS
- An **admin UI**: live traffic from clients to upstreams, the latest requests, and hosts and their rules to add and change
- Configuration through **HCL** with support for functions (get environment variables, etc)
- Powerful plugin system for adding new middlewares and other features using **WebAssembly (WASM)**
- Many others.

<img src="./docs/assets/admin/live-traffic.png" alt="The admin UI: a host's traffic as it happens, from clients to its locations and upstreams, with each one's requests a second, latency and errors" width="900">

See the [admin UI documentation](https://docs.proksi.info/admin-ui) for a tour of its pages.

# Quick start

1. Install Proksi:

```bash
# Option 1: Download the latest release for your system
# https://github.com/luizfonseca/proksi/releases

# Option 2: Install it with cargo
cargo install proksi

# Option 3: Run it with Docker
docker run -d -p 80:80 -p 443:443 -v /path/to/config:/etc/proksi/ luizfonseca/proksi
```

2. Create a configuration file named `proksi.hcl`
3. Add the following content to the file:

```hcl
lets_encrypt {
  enabled = true
  email = "my@email.com"
}

paths {
  # Where to save certificates?
  lets_encrypt = "./"
}

# The hosts Proksi should handle
# You might need to edit your /etc/hosts file here.
host "mysite.localhost" {
  # Will create a self-signed certificate for mysite.localhost
  tls {
    self_signed_on_failure = true
  }

  # Where to point mysite.localhost to
  location "/" {
    upstream "example.com:443" {
      sni = "example.com"
      headers {
        add = { "Host" = "example.com" }
      }
    }
  }
}
```

4. Run Proksi with your configuration:
```bash
# Option 1: Specify exact config file path
proksi -c /path/to/your/proksi.hcl
proksi -c /path/to/your/config.yml

# Option 2: Specify directory (looks for proksi.hcl, proksi.yml, proksi.yaml)
proksi -c /path/to/config-directory/

# Option 3: No config (uses minimal defaults with Let's Encrypt disabled)
proksi
```

## Configuration File Handling

Proksi supports flexible configuration file loading:

- **Direct file paths**: Specify any filename with a supported extension (`.hcl`, `.yml`, `.yaml`)
- **Directory paths**: Proksi will look for `proksi.hcl`, `proksi.yml`, or `proksi.yaml` in the specified directory
- **No configuration**: If no `-c` flag is provided, Proksi uses minimal default settings with Let's Encrypt disabled and emits a warning


# Configuration

Proksi is configured through **HCL** (recommended), **YAML** or **environment variables**. Below is everything you need to get going; the [documentation](https://docs.proksi.info) goes into more detail.

## Server

All of these are optional, the values below are the defaults:

```hcl
service_name = "proksi"

# Defaults to 80% of the available CPUs (at least 2)
worker_threads = 4

# Where hosts are served when they don't set `listen` addresses
server {
  https_address = "0.0.0.0:443"
  # HTTP requests are redirected to HTTPS (and used for Let's Encrypt challenges),
  # except for hosts with `tls = false`
  http_address = "0.0.0.0:80"

  # Behind a load balancer: the client of a request is the address it forwards in
  # X-Forwarded-For (for rate limits and access logs)
  # trusted_proxies = ["10.0.0.0/8"]

  # When stopping, open connections get this long to finish (proksi stops as soon as they
  # have). Give docker or kubernetes a little longer before they kill it, e.g.
  # `stop_grace_period: 35s` in compose, or `terminationGracePeriodSeconds: 35`.
  # shutdown_timeout_secs = 30

  # Timeouts toward clients, in seconds (these are the defaults)
  # timeouts {
  #   idle_secs  = 60 # waiting for a request's headers, and between requests of a connection
  #   read_secs  = 60 # waiting for each read of a request body, and for websocket messages
  #   write_secs = 60 # waiting for a client to read each write of a response
  # }
}

logging {
  enabled = true
  level = "info"             # debug, info, warn, error
  format = "json"            # json or pretty
  access_logs_enabled = true
  error_logs_enabled = true
  # path = "/var/log/proksi" # write logs to files instead of stdout
  # rotation = "daily"       # never, hourly, daily, minutely
}
```

## Hosts and locations

Every host Proksi accepts requests for is a `host` block. The label is matched against the `Host` header (case-insensitive), and it's also the name certificates are issued for.

Inside a host, `location` blocks decide where requests go based on their path:

```hcl
host "api.example.com" {
  location "/login" {
    upstream "auth:8080" {}
  }

  location "/" {
    upstream "app:8080" {}
  }
}
```

- The **most specific** location wins: `/login` beats `/`, a static path beats a parameter (`/users/:id`), which beats a wildcard (`/files/*`).
- A location matches its path **and everything below it**: `/login` serves `/login`, `/login/` and `/login/callback`, but not `/logins`.
- A request matching no location gets a `404`.

### Ports, plain HTTP, and any host

Each host decides where it's served with `listen`. Without it, a host is served on `server.https_address` (443), and HTTP requests are redirected to it. Proksi only opens the ports its hosts need.

A host with `tls = false` is served over plain HTTP (on `server.http_address` without `listen`), and no certificate is issued for it. This is useful behind another proxy or load balancer that already handles TLS, or when running Proksi on an IP address.

`host "*"` serves requests for any name that doesn't have a host of its own (it needs `tls = false`, as certificates are issued per name):

```hcl
# Plain HTTP on port 8080 for any name: Proksi doesn't open 443 at all
host "*" {
  tls = false
  listen = ["0.0.0.0:8080"]

  location "/" {
    upstream "app:8080" {}
  }
}

# HTTPS on 8443, only for this host
host "internal.example.com" {
  listen = ["10.0.0.1:8443"]

  location "/" {
    upstream "internal:3000" {}
  }
}
```

A port is either HTTP or HTTPS: a host with TLS and a host without it can't share an address. Hosts with TLS also need port 80 (`server.http_address`), for redirects and Let's Encrypt challenges.

## TCP

`tcp` blocks forward TCP connections, for services that don't speak HTTP. At the top level they are plain TCP; inside a host, TLS is terminated with the host's certificate (or forwarded untouched with `tls = "passthrough"`):

```hcl
# Plain TCP
tcp "game" {
  listen = "0.0.0.0:7777"
  upstream "10.0.0.1:7777" {}
}

# TLS with the certificate of db.example.com, plain TCP to the upstream
host "db.example.com" {
  tcp "postgres" {
    listen = "0.0.0.0:5433"
    upstream "10.0.0.5:5432" {}
  }
}
```

`tcp` blocks can also share the HTTPS port: with `listen = "0.0.0.0:443"`, connections asking for the host's name are forwarded while every other name is still HTTPS. If the host also has locations, `alpn = ["..."]` tells them apart by the protocol the client asks for. See the [TCP documentation](https://docs.proksi.info/routing/tcp) for more.

## Upstreams

Upstreams are where requests are sent. They are labelled by their address (`host:port`, or `[ipv6]:port`) and requests are load balanced between them (round-robin, in proportion to their `weight`), with health checks running in the background:

```hcl
host "app.example.com" {
  location "/" {
    upstream "10.0.0.1:3000" {
      weight = 2 # twice the requests of the others
    }
    upstream "10.0.0.2:3000" {}
  }

  # An external service using TLS
  location "/docs" {
    upstream "example.com:443" {
      sni = "example.com"

      # Headers added to the request sent to the upstream
      headers {
        add = { "Host" = "example.com" }
      }
    }
  }
}
```

## Certificates

With Let's Encrypt enabled, Proksi issues and renews a certificate for every host automatically:

```hcl
lets_encrypt {
  enabled = true
  email = "my@email.com"
  # Use the staging environment while testing, then switch to `false`
  staging = true
}

paths {
  lets_encrypt = "/etc/proksi/letsencrypt"
}

host "example.com" {
  location "/" {
    upstream "app:8080" {}
  }
}
```

You can also bring your own certificate, or fall back to a self-signed one (useful for development):

```hcl
host "example.com" {
  tls {
    certificate {
      pem = "/etc/proksi/certs/example.com.pem"
      key = "/etc/proksi/certs/example.com.key"
    }

    self_signed_on_failure = true
  }

  location "/" {
    upstream "app:8080" {}
  }
}
```

The `pem` file holds the certificate followed by its intermediates (a "fullchain"), which proksi sends to clients along with it. Both files are checked every 10 seconds: a renewed certificate is served without restarting proksi (a certificate and a key that don't match yet, while they're being replaced, are skipped until they do).

## Headers

Headers can be added or removed from responses, for a whole host or a single location (a location's headers come after the host's):

```hcl
host "example.com" {
  headers {
    response {
      add = { "X-Api-Version" = "2" }
      remove = ["Server"]
    }
  }

  location "/" {
    upstream "app:8080" {}
  }
}
```

## Plugins

Built-in plugins are enabled per location, and run in order of their `priority` (see the [HCL documentation](https://docs.proksi.info/configuration/hcl)):

```hcl
host "example.com" {
  location "/" {
    upstream "app:8080" {}

    # Adds an X-Request-Id header to requests and responses
    request_id {}

    # Only these clients (addresses or networks), the others get a 403
    ip_filter {
      allow = ["10.0.0.0/8"]
    }

    # Pages of these origins can call the location from the browser
    cors {
      origins = ["https://app.example.com"]
    }

    # 120 requests per minute for each client (IP address), then a 429
    rate_limit {
      requests = 120
      per      = "1m"
    }

    # Request bodies over 1MiB get a 413
    body_limit {
      max = "1MiB"
    }

    # Responses compressed with zstd, brotli or gzip, as the client accepts
    compress {}
  }
}
```

Locations can also require their clients to sign in:

```hcl
host "example.com" {
  # A user and password, from environment variables
  location "/admin" {
    upstream "app:8080" {}

    basic_auth {
      user = env("BASIC_AUTH_USER")
      pass = env("BASIC_AUTH_PASS")
    }
  }

  # Or ask an auth service (Authelia, Authentik, oauth2-proxy) about each request
  location "/internal" {
    upstream "app:8080" {}

    forward_auth {
      url              = "http://authelia:9091/api/authz/forward-auth"
      response_headers = ["Remote-User"]
    }
  }

  # Or sign in with GitHub
  location "/" {
    upstream "app:8080" {}

    oauth2 {
      provider      = "github"
      client_id     = env("OAUTH_CLIENT_ID")
      client_secret = env("OAUTH_CLIENT_SECRET")
      jwt_secret    = env("OAUTH_JWT_SECRET")
      validations   = [{ key = "email", value = ["me@example.com"] }]
    }
  }
}
```

Locations can also answer without an upstream, with a [`redirect`](https://docs.proksi.info/plugins/redirect) (`to = "https://example.com{uri}"`) or a fixed response ([`respond`](https://docs.proksi.info/plugins/respond), such as `status = 503` and `body = "Back soon"`).

## Rules

A rule is a condition on a request, and what to do when it holds. Rules go in the `server`, a `host` or a `location`, and run in order:

```hcl
host "shop.example.com" {
  location "/api" {
    upstream "api:4000" {}

    # Reads stay unlimited; writes are limited per client
    rule "writes" {
      and {
        method {
          not_in = ["GET", "HEAD", "OPTIONS"]
        }
      }
      then {
        rate_limit {
          requests = 60
          per      = "1m"
        }
      }
    }
  }
}
```

Conditions combine with `and`, `or` and `not`, on the method, host, path, query, headers, cookies or client; `then` takes the blocks a location takes, and can change the request's method and headers. See the [rules documentation](https://docs.proksi.info/rules).

## Caching

Responses can be cached in memory or on disk:

```hcl
host "example.com" {
  location "/" {
    upstream "app:8080" {}

    cache {
      enabled = true
      cache_type = "disk"        # or "memcache"
      path = "/tmp/proksi-cache"
      expires_in_secs = 3600
    }
  }
}

# Memory used by the disk cache to keep popular responses (default: 256MB)
cache {
  memory_limit_mb = 256
}
```

## Docker

Proksi can find your containers (or Docker Swarm services) through labels, without touching the configuration file:

```hcl
docker {
  enabled = true
  mode = "container" # or "swarm"
  endpoint = "unix:///var/run/docker.sock"
  interval_secs = 15
}
```

```yaml
services:
  web:
    image: nginxdemos/hello
    labels:
      proksi.enabled: "true"
      proksi.host: "mysite.localhost"
      proksi.port: "80"
      proksi.ssl_certificate.self_signed_on_failure: "true"
```

Routes from labels are kept when a container stops (requests get a `503`) and are removed when Proksi restarts.

## Kubernetes

Proksi can run as a Kubernetes ingress controller, serving the `Ingress` resources of class `proksi` and sending requests straight to the ready pods of each service:

```bash
helm install proksi deploy/helm/proksi --namespace proksi --create-namespace
# or: kubectl apply -f https://raw.githubusercontent.com/luizfonseca/proksi/main/deploy/kubernetes/proksi.yaml
```

See the [Kubernetes documentation](https://docs.proksi.info/installation/kubernetes) for paths, TLS and running more than one replica.

## Running many servers

To run more than one Proksi server with the same routes and certificates, use a shared store. Routes in the store take priority over the configuration file and docker labels, and every server syncs them every `sync_interval_secs`:

```hcl
store {
  store_type = "redis"   # memory (default), redis, sqlite, postgres
  redis_url = "redis://localhost:6379"
  # database_url = "postgres://user:pass@localhost/proksi" # for sqlite and postgres
  sync_interval_secs = 10
}
```

SQLite and Postgres need Proksi to be built with the `store-sqlite` / `store-postgres` features.

## Metrics

With an `admin` block, Proksi serves Prometheus metrics (requests, latency and upstream errors per host, location and upstream) on an address of its own:

```hcl
admin {
  listen = "127.0.0.1:7555"

  # sent as `Authorization: Bearer <secret>`; read-only unless `access = "write"`
  token "grafana" {
    secret = env("PROKSI_ADMIN_TOKEN")
  }
}
```

Without a `token` block, `listen` must be a loopback address. The same address serves a UI, which signs in with a token. Live numbers (requests per second, p50/p90/p99 latency, errors and upstream health over the last minutes) are at `/api/v1/stats`, or streamed every second from `/api/v1/stats/stream`. See the [admin documentation](https://docs.proksi.info/configuration/admin) for the metrics and a Prometheus configuration.

## Auto reload

Proksi can watch the configuration file and apply changes on the fly. Changes to hosts are applied without restarting; an invalid file is ignored (and logged), so Proksi keeps running:

```hcl
auto_reload {
  enabled = true
  interval_secs = 30
  # Extra paths to watch (e.g. files used with import())
  paths = ["/etc/proksi/sites"]
}
```

## Functions and environment variables

HCL configurations can use a few functions:

- `env("NAME")`: reads an environment variable (fails if it's not set)
- `import("./sites/example.com.hcl")`: includes another HCL file
- `num_cpus()`: the number of CPUs available

```hcl
# file: sites/example.com.hcl
location "/" {
  upstream "app:8080" {}
}
```

```hcl
worker_threads = num_cpus()

# One file per host
host = {
  "example.com" = import("./sites/example.com.hcl")
}
```

Every setting can also be set (and overridden) through environment variables starting with `PROKSI_`, using `__` for nested keys, e.g. `PROKSI_LOGGING__LEVEL=debug` or `PROKSI_LETS_ENCRYPT__EMAIL=my@email.com`.

## YAML

The same configuration in YAML:

```yaml
lets_encrypt:
  enabled: true
  email: "my@email.com"

host:
  api.example.com:
    location:
      /login:
        upstream:
          "auth:8080": {}
      /:
        upstream:
          "app:8080": {}
```

## Migrating from `routes = [...]`

`routes = [...]` still works but is deprecated (Proksi logs a warning). The [HCL documentation](https://docs.proksi.info/configuration/hcl) shows how each route becomes a `host` block.


## What's next

See the [features](https://docs.proksi.info/general/features) page for what's available today.


# Documentation
Documentation for Proksi can be found at [https://docs.proksi.info](https://docs.proksi.info) which is also available in the [docs](./docs/) folder of this repository (served by [docs.page](https://docs.page)).


# Contributing
We welcome contributions to Proksi. If you have any **suggestions** or **ideas**, please feel free to open an issue or a pull request on the GitHub repository.

# License
Proksi is licensed under the [MIT License](https://github.com/luizfonseca/proksi/blob/main/LICENSE), the [Apache License 2.0](https://github.com/luizfonseca/proksi/blob/main/LICENSE-APACHE) and is free to use and modify.
