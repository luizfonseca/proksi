# YAML

Proksi can be configured using a proksi.yaml file that controls most of the functions:



<table><thead><tr><th width="311.3333333333333">Property</th><th width="268">Description</th><th>Default</th></tr></thead><tbody><tr><td><code>service_name</code></td><td>Name of the service. It's used for logging.</td><td>"proksi"</td></tr><tr><td><code>worker_threads</code></td><td>Number of (real) threads the HTTPs service will use.</td><td>4</td></tr><tr><td><code>lets_encrypt</code></td><td>--</td><td>--</td></tr><tr><td><code>lets_encrypt.enabled</code></td><td>Enables issuing certificates from Let's Encrypt</td><td>true</td></tr><tr><td><code>lets_encrypt.email</code></td><td>The email to be used when asking for certificates</td><td>""</td></tr><tr><td><code>lets_encrypt.staging</code></td><td>Use the <code>staging</code> endpoint to generate certificates. Mostly useful for local testing. Change it to <code>true</code> to enable the production certificates.</td><td>true</td></tr><tr><td><code>logging</code></td><td>--</td><td>--</td></tr><tr><td><code>logging.level</code></td><td>The level of logs saved or printed to STDOUT.</td><td>INFO</td></tr><tr><td><code>logging.access_logs_enabled</code></td><td>Enables response/request logging (includes user-agent, host, duration etc)</td><td>true</td></tr><tr><td><code>logging.error_logs_enabled</code></td><td>If the logs should include errors from Pingora</td><td>false</td></tr><tr><td><code>paths</code></td><td>--</td><td>--</td></tr><tr><td><code>paths.lets_encrypt</code></td><td>Path to store certificates, challenges etc</td><td>"/etc/proksi/lets_encrypt"</td></tr><tr><td><code>routes</code></td><td>--</td><td>--</td></tr><tr><td><code>routes[*].host</code></td><td>The host name that a list of upstreams will receive requests for</td><td></td></tr><tr><td><code>routes[*].path_prefix</code></td><td>Will match host+path on every request ensuring that only requests where the <code>path</code> starts with the value defined here are matched.</td><td></td></tr><tr><td><code>routes[*].upstreams</code></td><td>--</td><td>--</td></tr><tr><td><code>routes[*].upstreams[*].ip</code></td><td>The IP of your server, container, or <strong>even an external IP</strong> you want to point requests to.</td><td></td></tr><tr><td><code>routes[*].upstreams[*].port</code></td><td>The <code>PORT</code> of your server, container or external service where we should connect to.</td><td></td></tr><tr><td><code>routes[*].upstreams[*].network</code></td><td>The network name for Proksi to use when connecting with internal services or containers</td><td></td></tr><tr><td></td><td></td><td></td></tr></tbody></table>



## Example file

Below you can find a file with the current defaults and&#x20;

```yaml
# Description: Example configuration file for Proksi
#
# Proksi is a reverse proxy server that can be used to route incoming requests 
# to different upstream servers based on the request's host, path, headers, and 
# other attributes.
#
# This configuration file specifies the following settings:
#
#
# ------------------------------------------------------------------
# The name of the service is "proksi".
# This will show in logs and it's mostly used for log filtering if needed
service_name: "proksi"

# Number of threads that the HTTPS service will use to handle incoming requests.
# This can be adjusted based on the number of CPU cores available on the server.
# The default value is 1.
#
# Note: Increasing the number of threads can improve the performance of the server, 
# but it can also increase the memory usage.
#
# Note 2: This only affect the HTTPS service, the HTTP service
# (and other background services) is single threaded.
worker_threads: 4

# The configuration for the Let's Encrypt integration.
lets_encrypt:

  # Whether the Let's Encrypt integration is enabled
  # (the background service will run and issue certificates for your routes).
  enabled: true

  # The email address to use for Let's Encrypt notifications and account registration.
  # Important: Make sure to replace this with your own email address.
  # any "@example.com" is invalid and will not work.
  email: "your-email@example.com"

  # The staging flag is used to test the Let's Encrypt integration without hitting the rate limits.
  # When set to <true>, the integration will use the Let's Encrypt staging environment.
  # --
  # When set to <false>, the integration will use the Let's Encrypt production environment
  # and certificates will be publicly trusted for 90 days.
  staging: true

# The logging configuration for the server.
logging:
  # The log level for the server (can be "DEBUG", "INFO", "WARN", "ERROR").
  level: "INFO"

  # Whether access logs are enabled.
  # When set to <true>, the server will log information about incoming requests.
  # This information includes the request method, path, status code, response time and more.
  access_logs_enabled: true

  # Whether error logs are enabled.
  error_logs_enabled: false

# The paths for the TLS certificates, challenges, orders, and account credentials.
# You can override any, these are the current defaults.
paths:

  # The path where the TLS certificates will be stored.
  # If the path doesn't exist, it will be created if the binary has the right permissions.
  lets_encrypt: "/etc/proksi/certificates"


# The list of routes that the server will use to route incoming requests
# to different upstream servers.
# Each route is an item in the list and it has the following attributes:
routes:

  # The host attribute specifies the hostname that the route will match.
  # This is normally the domain, subdomain that you want to route to a particular server/ip.
  # This can be a domain name or an IP address. For IP address, no certificate will be issued.
  # The host attribute is required.
  - host: "example.com"

    # The path_prefix attribute specifies the path prefix that the route will match.
    path_prefix: "/api"

    # The headers attribute specifies the headers that will
    # be added or removed at the end of the response
    # --
    # In the near future you will be able to modify the headers
    # of the request send to the upstream server
    headers:
      # Adds the given headers to the dowstream (client) response
      add:
        - name: "X-Forwarded-For"
          value: "<value>"
        - name: "X-Api-Version"
          value: "1.0"
      # Removes the given headers from the dowstream (client) response
      remove:
        - name: "Server"
    # The upstreams attribute specifies the list of upstream servers that the route will use.
    # These are load balanced and the server will try to connect to the first one in the list.
    # If the connection fails, it will try the next one.
    # --
    # Health checks run in the background to ensure you have a healthy connection always.
    upstreams:
      # The IP address of the upstream server
      # (can be any IP address, as long as Proksi can access it).
      - ip: "10.1.2.24/24"
        # The port of the upstream server (can be any port).
        port: 3000

        # The network attribute specifies the network that the upstream server is part of.
        # This is mostly important for Docker containers, but it can be used for other purposes.
        network: "public"
      - ip: "10.1.2.23/24"
        port: 3000
        network: "shared"

```
