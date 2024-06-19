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

  # Formats "json" or "pretty""
  format = "pretty"
}

# The paths for the TLS certificates, challenges, orders, and account credentials.
# You can override any, these are the current defaults.
paths {

  # The path where the TLS certificates will be stored.
  # If the path doesn't exist, it will be created if the binary has the right permissions.
  lets_encrypt = "/etc/proksi/letsencrypt"
}

routes = [
  {
    # The host attribute specifies the hostname that the route will match.
    # This is normally the domain, subdomain that you want to route to a particular server/ip.
    # This can be a domain name or an IP address. For IP address, no certificate will be issued.
    # The host attribute is required.
    host = "example.com"


    # The upstreams attribute specifies the list of upstream servers that
    # the route will use.
    # These are load balanced and the server will try to connect to
    # the first one in the list.
    # If the connection fails, it will try the next one.
    # --
    # Health checks run in the background to ensure you have a healthy connection always.
    upstreams = [
      {
        # The IP address of the upstream server or HOSTNAME.
        ip = "google.com"
        # The network attribute specifies the network that the upstream server is part of.
        # This is mostly important for Docker containers, but it can be used for other purposes.
        # network = "public"
        port = 443
        sni = "google.com"

        # The headers attribute specifies the headers that will
        # be added or removed when making a request to your UPSTREAM (your server)
        headers = {
          add = {
            name = "Host"
            value = "google.com"
          }
        }
      },
      # New upstream
      {
        ip = "10.1.2.23/24"
        network = "shared"
        port = 3000
      }
    ]


    # The headers attribute specifies the headers that will
    # be added or removed at the end of the response to DOWNSTREAM (client)
    headers = {
      add = [
        {  name = "X-Forwarded-For", value = "<value>" },
        {  name = "X-Api-Version", value = "<value>" }
      ]

      remove = [{  name = "Server" }]
    }

    # SSL configuration for the route.
    # The ssl attribute is optional.
    ssl = {
      path = {
        key = "/etc/proksi/certs/my-host.key"
        pem = "/etc/proksi/certs/my-host.pem"
      }

      self_signed_fallback = true
    }

    // DEPRECATED
    # ssl_certificate = {
    #   self_signed_on_failure = true
    # }

    # Match a given request path with the route.
    # You can have multiple matchers:
    # Path related
    # Header related
    match_with = {
      path = {
        patterns = ["/api/*", "/*"]
      }
    }

    # Plugins that will be applied to the route/host
    # (ex: rate limiting, oauth2, etc.)
    # Plugins can be used to extend the functionality of Proksi.
    # For example, the oauth2 plugin can be used to authenticate users using OAuth2 providers.
    plugins = [
    { name:  "request_id" },
    { name:  "basic_auth", config: { user: "<user>", pass: "<pass>" } },
    { name:  "oauth2", config: {
      provider: "github",
      client_id: "<client_id>",
      client_secret: "<client_secret>",
      jwt_secret: "<jwt_secret>",
      validations: [ { key: "team_id", value: [ "<team_id>" ] } ] }
    }
  }
]
