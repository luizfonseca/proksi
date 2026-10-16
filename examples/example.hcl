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

    # Plugins (optional)
    request_id {}

    oauth2 {
      provider      = "github"
      client_id     = "<client_id>"
      client_secret = "<client_secret>"
      jwt_secret    = "<jwt_secret>"
      validations   = [{ key = "team_id", value = ["<team_id>"] }]
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

# TCP connections forwarded as they are, for services that don't speak HTTP.
# Inside a host, `tcp` blocks use the host's certificate (see docs/routing/tcp.md).
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
