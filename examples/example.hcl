# Description: Example configuration file for Proksi
#
# Proksi is a reverse proxy server that can be used to route incoming requests to different upstream servers based on the request's host, path, headers, and other attributes.
#
# This configuration file specifies the following settings:
service_name = "proksi"

# Number of threads that the HTTPS service will use to handle incoming requests.
# This can be adjusted based on the number of CPU cores available on the server.
# The default value is 1.
#
# Note: Increasing the number of threads can improve the performance of the server,
# but it can also increase the memory usage.
#
# Note 2: This only affect the HTTPS service, the HTTP service
# (and other background services) is single threaded.
worker_threads = 4


# The server block specifies the settings for the HTTP/HTTPS server.
server {
  # The address that the server will listen on while serving HTTPS.
  # This can be a TCP address or a Unix socket.
  # The default value is "0.0.0.0:443".
  https_address = "0.0.0.0:443"

  # The address that the server will listen on for HTTP requests.
  # This can be a TCP address or a Unix socket.
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
  # redis_url = "redis://localhost:6479/"
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
  # (the background service will run and issue certificates for your routes).
  enabled = true

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
