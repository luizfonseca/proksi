# Description: Example configuration file for Proksi
#
# Proksi is a reverse proxy server that can be used to route incoming requests to different upstream servers based on the request's host, path, headers, and other attributes.
#
# This configuration file specifies the following settings:
service_name = "proksi"

# Number of threads used to handle requests (on the HTTPS and HTTP ports).
# This can be adjusted based on the number of CPU cores available on the server.
# The default value is 80% of the available CPUs (at least 2).
#
# Note: Increasing the number of threads can improve the performance of the server,
# but it can also increase the memory usage.
worker_threads = 4


# The server block specifies the default addresses of the server.
# Hosts are served on these addresses unless they set their own (see `listen` below).
# Proksi only opens the ports its hosts need.
server {
  # Where hosts with TLS are served.
  # The default value is "0.0.0.0:443".
  https_address = "0.0.0.0:443"

  # Where hosts without TLS are served. Hosts with TLS also use it to redirect
  # HTTP requests to HTTPS and to answer Let's Encrypt challenges.
  # The default value is "0.0.0.0:80".
  http_address = "0.0.0.0:80"

  # Proxies or load balancers in front of Proksi (optional). The client of a request is then
  # the address they forward in `X-Forwarded-For` (rate limits, ip_filter, access logs).
  trusted_proxies = ["10.0.0.0/8"]

  # How long Proksi waits for clients, and for upstreams (the defaults).
  # Hosts and locations can change them for their requests.
  timeouts {
    idle_secs  = 60 # for a request's headers, and between requests of a connection
    read_secs  = 60 # for each read of a request body
    write_secs = 60 # for a client to read each write of a response

    upstream {
      connect_secs = 5   # to connect
      tls_secs     = 10  # for the TLS handshake of an HTTPS upstream
      read_secs    = 360 # for each read of a response, including its headers
      write_secs   = 60  # for the upstream to read each write of a request
      idle_secs    = 360 # to keep an unused connection open, to reuse it
    }
  }

  # TLS versions and TLS 1.2 ciphers clients can use (optional).
  tls {
    min_version = "1.2"
  }

  # Rules of every request, before its host is chosen (see docs/rules).
  # Here: an old domain moved to a new one, keeping every path.
  rule "old domain" {
    and {
      host {
        eq = "old.example.com"
      }
    }
    then {
      redirect {
        to     = "https://example.com{uri}"
        status = 301
      }
    }
  }
}

# The admin API, metrics and UI (optional: off without this block).
admin {
  listen = "127.0.0.1:7555"

  # Every request needs a token's secret (`Authorization: Bearer <secret>`).
  # Tokens can read unless they have `access = "write"` (to change hosts in the store).
  token "grafana" {
    secret = "<read_token>"
  }

  token "ops" {
    secret = "<write_token>"
    access = "write"
  }
}


# The store block specifies the settings for the store
# You can store all proksi configuration such as certificates, routes, pathing etc.
store {
  # Defaults to use in-memory storage.
  store_type = "memory"

  # Change to get data from redis
  # store_type = "redis"
  # redis_url = "redis://localhost:6379/"

  # Or from SQLite / Postgres (requires the `store-sqlite` / `store-postgres` features)
  # store_type = "postgres"
  # database_url = "postgres://user:pass@localhost/proksi"

  # How often (in seconds) routes are synced from the store.
  # sync_interval_secs = 10
}

docker {
  # Whether the Docker integration is enabled
  # (the background service will run and listen for Docker events).
  # Default value is <false>.
  enabled = false

  # The endpoint for the Docker API.
  # This can be a TCP endpoint or a Unix socket.
  # The default value is "unix:///var/run/docker.sock".
  endpoint = "unix:///var/run/docker.sock"

  # The interval in seconds at which the Docker integration will check for new containers.
  # The default value is 15 seconds.
  # important: the lower the value, the more work Proksi has to do.
  interval_secs = 15

  # The mode of the Docker integration.
  # The mode can be "container" or "swarm".
  # When the mode is set to "container", the integration will only
  # listen for labels in Docker containers
  # When the mode is set to "swarm", the integration will listen for Docker
  # events in a Docker Swarm cluster (service labels).
  # Default value is <container>.
  mode = "container"
}

lets_encrypt {
  # Whether the Let's Encrypt integration is enabled
  # (the background service will run and issue certificates for your hosts).
  # Note: You must explicitly enable this and provide a valid email address.
  enabled = false

  # The email address to use for Let's Encrypt notifications and account registration.
  # Important: Make sure to replace this with your own email address.
  # any "@example.com" is invalid and will not work.
  email = "your-email@example.com"

  # The staging flag is used to test the Let's Encrypt integration without hitting the rate limits.
  # When set to <true>, the integration will use the Let's Encrypt staging environment.
  # --
  # When set to <false>, the integration will use the Let's Encrypt production environment
  # and certificates will be publicly trusted for 90 days.
  staging = true

  # Where DNS-01 challenges are written (optional): needed by wildcard hosts
  # (`*.example.com`) and hosts with `challenge = "dns-01"`.
  # Proksi writes a TXT record at `_acme-challenge.<name>`, and removes it once checked.
  dns {
    # The only provider for now.
    provider = "cloudflare"

    # An API token limited to `Zone:DNS:Edit` on the zones Proksi serves.
    api_token = "<cloudflare_api_token>"
  }
}

logging  {
  # Whether to log anything at all (default: true)
  enabled = true

  # The log level for the server (can be "DEBUG", "INFO", "WARN", "ERROR").
  level = "INFO"

  # Whether access logs are enabled.
  # When set to <true>, the server will log information about incoming requests.
  # This information includes the request method, path, status code, response time and more.
  access_logs_enabled = true

  # Whether error logs are enabled.
  error_logs_enabled = false

  # Formats "json" or "pretty"
  format = "pretty"

  # The path where the log files will be stored.
  path = "/tmp"

  # The rotation mode for the log files.
  # The rotation mode can be "daily", "hourly", "minutely" or "never".
  # Files will be generated with the particular rotation mode as the suffix.
  rotation = "daily"
}

# The paths for the TLS certificates, challenges, orders, and account credentials.
# You can override any, these are the current defaults.
paths {

  # The path where the TLS certificates will be stored.
  # If the path doesn't exist, it will be created if the binary has the right permissions.
  lets_encrypt = "/etc/proksi/letsencrypt"
}

# Hosts that the server will accept requests for.
# The label is the hostname (or IP address) matched against the Host header of each request,
# and used to issue certificates when Let's Encrypt is enabled.
host "example.com" {
  # The addresses this host is served on (optional).
  # Defaults to `server.https_address` (or `server.http_address` when `tls = false`).
  # listen = ["0.0.0.0:8443"]

  # TLS settings of the host (optional).
  tls {
    # Certificate files, used instead of Let's Encrypt.
    certificate {
      key = "/etc/proksi/certs/my-host.key"
      pem = "/etc/proksi/certs/my-host.pem"
    }

    # Use a self-signed certificate if no certificate can be obtained.
    self_signed_on_failure = true
  }

  # Headers added or removed from every response of the host (to the client).
  headers {
    response {
      add    = { "X-Api-Version" = "<value>" }
      remove = ["Server"]
    }
  }

  # How the upstreams of its locations are checked (default: every 15 seconds).
  health_check {
    interval_secs = 5
  }

  # Timeouts of the host's requests, instead of the server's.
  timeouts {
    upstream {
      connect_secs = 2
    }
  }

  # Rules of the host's requests, before their location is chosen.
  rule "maintenance" {
    and {
      path {
        prefix = "/checkout"
      }
    }
    then {
      respond {
        status = 503
        body   = "Checkout is back in a few minutes"
      }
    }
  }

  # Locations choose how requests are handled based on their path.
  # The most specific location wins: `/api` serves `/api`, `/api/` and everything
  # below it (not `/apis`), and `/` serves whatever no other location matches.
  location "/api" {
    # Upstreams are labelled by their address (host:port or [ipv6]:port).
    # Requests are load balanced between them (round-robin), with health checks
    # running in the background.
    upstream "10.1.2.23:3000" {
      # The docker network to reach the upstream through (optional).
      network = "shared"
    }

    upstream "10.1.2.24:3000" {}

    # Plugins (optional). They run by priority: see docs/configuration/hcl.
    request_id {}

    # Requests per client, in each period.
    rate_limit {
      requests = 120
      per      = "1m"
    }

    # Cross-origin requests from these pages.
    cors {
      origins      = ["https://app.example.com", "https://*.preview.example.com"]
      credentials  = true
      max_age_secs = 600
    }

    # The largest request body accepted (a 413 otherwise).
    body_limit {
      max = "1MiB"
    }

    # Responses compressed (zstd, brotli or gzip), as the client accepts.
    compress {}

    # The path sent to the upstreams: /api/users is sent as /users.
    rewrite {
      strip_prefix = "/api"
    }

    # Timeouts of this location, instead of the host's.
    timeouts {
      read_secs = 300 # large uploads from slow connections
    }

    oauth2 {
      provider      = "github"
      client_id     = "<client_id>"
      client_secret = "<client_secret>"
      jwt_secret    = "<jwt_secret>"
      validations   = [{ key = "team_id", value = ["<team_id>"] }]
    }
  }

  # Only from the office and the VPN, and only for users the auth service lets through.
  location "/admin" {
    upstream "10.1.2.25:3000" {}

    ip_filter {
      allow = ["203.0.113.0/24", "10.8.0.0/16"]
      deny  = ["10.8.0.13"]
    }

    # Each request is checked by an auth service first (Authelia, Authentik...).
    forward_auth {
      url              = "http://authelia:9091/api/authz/forward-auth"
      response_headers = ["Remote-User", "Remote-Groups"]
    }
  }

  # Responses kept by Proksi and served again while fresh.
  location "/images" {
    upstream "10.1.2.26:3000" {}

    cache {
      enabled         = true
      cache_type      = "memcache" # or "disk", with `path`
      expires_in_secs = 3600
    }
  }

  # Answered by Proksi itself.
  location "/robots.txt" {
    respond {
      body         = "User-agent: *\nDisallow: /admin\n"
      content_type = "text/plain"
    }
  }

  location "/" {
    upstream "google.com:443" {
      # The server name sent to an upstream using TLS.
      sni = "google.com"

      # Headers added to the request sent to the upstream.
      headers {
        add = { "Host" = "google.com" }
      }
    }

    basic_auth {
      user = "<user>"
      pass = "<pass>"
    }

    # Headers of this location, added after the host's.
    headers {
      response {
        add = { "X-Location" = "root" }
      }
    }
  }
}

# A host with an app and its API, showing the blocks of a location.
host "app.example.com" {
  # The app's files, and its index.html for the paths it routes itself.
  location "/" {
    static_files {
      # An absolute path.
      directory = "/var/www/app/dist"

      # Served for paths with no file (single-page apps). Without it: a 404.
      fallback = "/index.html"

      # The `Cache-Control` of the files (optional).
      cache_control = "public, max-age=300"
    }
  }

  location "/api" {
    upstream "10.1.2.50:8080" {}
    upstream "10.1.2.51:8080" { weight = 2 }

    # How each request's upstream is chosen: "round_robin" (default), or
    # "least_connections": the one with the fewest requests in progress for its weight.
    load_balancing = "least_connections"

    # Requests sent to another upstream when one fails (optional).
    retry {
      # Tries after the first (default 1, at most 10).
      attempts = 2

      # "connect": the upstream couldn't be reached (any request is tried again).
      # Statuses: only idempotent requests (GET, HEAD, PUT, DELETE, OPTIONS) are tried again.
      on = ["connect", "502", "503", "504"]
    }

    # Requests need a valid JSON Web Token (`Authorization: Bearer <token>`). Others get a 401.
    jwt {
      # The issuer's public keys. Or `secret` (HMAC), or `public_key` (PEM).
      jwks_url = "https://auth.example.com/.well-known/jwks.json"
      issuer   = "https://auth.example.com/"
      audience = ["api"]

      # Claims sent to the upstream as headers (the client's own are removed).
      claims_to_headers = { sub = "X-User-Id" }
    }

    # A share of the clients sent to other upstreams: a canary, an A/B test.
    # Each client keeps its side, by a cookie telling nothing about it.
    traffic_split "v2" {
      percent = 10

      # "cookie" (default), "client_ip", or "none" (each request on its own).
      sticky = "cookie"

      upstream "10.1.2.60:8080" {}
    }

    # Copies of the requests sent to other upstreams, whose answers are dropped:
    # requests never wait on them (shadow traffic).
    mirror {
      # The share of requests copied (default 100).
      percent = 5

      # Requests with a larger body aren't copied (default "64KiB").
      max_body_size = "64KiB"

      # Copies being sent at most, and how long each can take (defaults 100 and 5).
      max_in_flight = 100
      timeout_secs  = 5

      upstream "10.1.2.70:8080" {}
    }

    # Rules choose what happens to the requests they match (see docs/rules).
    rule "beta testers" {
      and {
        cookie "beta" {
          eq = "1"
        }
      }
      then {
        # Whatever their share: to the split, and copied to the mirror.
        traffic_split = "v2"
        mirror        = true
      }
    }

    rule "payments" {
      and {
        method {
          eq = "POST"
        }
        path {
          prefix = "/api/payments"
        }
      }
      then {
        # Never sent twice, and never copied.
        retry  = false
        mirror = false
      }
    }
  }
}

# Names one label under example.com (`a.example.com`, not `a.b.example.com`).
# A host with the exact name wins over it. Its certificate is a wildcard one,
# from Let's Encrypt with a DNS challenge (see `lets_encrypt.dns`) or from certificate files.
host "*.example.com" {
  tls {
    challenge = "dns-01"
  }

  location "/" {
    upstream "10.1.2.80:3000" {}
  }
}

# Requests sent elsewhere: www.example.com/a?b=1 goes to https://example.com/a?b=1.
host "www.example.com" {
  location "/" {
    redirect {
      to     = "https://example.com{uri}"
      status = 301
    }
  }
}

# TLS forwarded untouched to an upstream that terminates it with its own certificate.
host "secure.example.com" {
  tcp "app" {
    listen = "0.0.0.0:8443"
    tls    = "passthrough"
    upstream "10.1.2.90:443" {}
  }
}

# TCP connections forwarded as they are, for services that don't speak HTTP.
# Inside a host, `tcp` blocks use the host's certificate (see docs/routing/tcp.mdx).
tcp "game" {
  listen = "0.0.0.0:7777"

  upstream "10.1.2.40:7777" { weight = 2 }
  upstream "10.1.2.41:7777" {}
}

# A host served over plain HTTP, without certificates.
# Useful behind another proxy or load balancer that already handles TLS.
host "internal.example.com" {
  tls = false

  # Served only on this address (Proksi opens it at startup).
  listen = ["10.0.0.1:8080"]

  location "/" {
    upstream "10.1.2.30:3000" {}
  }
}

# Any other name: requests whose Host header doesn't match a host of their own.
# It needs `tls = false`, as certificates are issued per name.
host "*" {
  tls = false

  location "/" {
    upstream "10.1.2.31:3000" {}
  }
}
