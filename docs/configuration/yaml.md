# YAML

Proksi can be configured using YAML files. You have several options for specifying your configuration:

## Configuration File Loading

Proksi supports flexible configuration file loading via the `-c` flag:

- **Direct file path**: `proksi -c /path/to/my-config.yml` - Load any YAML file directly
- **Directory path**: `proksi -c /path/to/directory/` - Proksi looks for `proksi.yaml`, `proksi.yml`, or `proksi.hcl` in the directory
- **No configuration**: `proksi` (without `-c`) - Uses minimal default configuration with Let's Encrypt disabled

When no configuration is found, Proksi will emit a warning and start with minimal defaults that include:
- Let's Encrypt disabled (to prevent email validation failures)
- Basic HTTP/HTTPS proxy functionality
- No predefined hosts

## Configuration Properties



<table><thead><tr><th width="311.3333333333333">Property</th><th width="268">Description</th><th>Default</th></tr></thead><tbody><tr><td><code>service_name</code></td><td>Name of the service. It's used for logging.</td><td>"proksi"</td></tr><tr><td><code>worker_threads</code></td><td>Number of (real) threads the HTTPs service will use.</td><td>80% of the available CPUs (at least 2)</td></tr><tr><td><code>lets_encrypt</code></td><td>--</td><td>--</td></tr><tr><td><code>lets_encrypt.enabled</code></td><td>Enables issuing certificates from Let's Encrypt</td><td>false</td></tr><tr><td><code>lets_encrypt.email</code></td><td>The email to be used when asking for certificates</td><td>""</td></tr><tr><td><code>lets_encrypt.staging</code></td><td>Use the <code>staging</code> endpoint to generate certificates. Mostly useful for local testing. Change it to <code>true</code> to enable the production certificates.</td><td>true</td></tr><tr><td><code>logging</code></td><td>--</td><td>--</td></tr><tr><td><code>logging.level</code></td><td>The level of logs saved or printed to STDOUT.</td><td>INFO</td></tr><tr><td><code>logging.access_logs_enabled</code></td><td>Enables response/request logging (includes user-agent, host, duration etc)</td><td>true</td></tr><tr><td><code>logging.error_logs_enabled</code></td><td>If the logs should include errors from Pingora</td><td>false</td></tr><tr><td><code>paths</code></td><td>--</td><td>--</td></tr><tr><td><code>paths.lets_encrypt</code></td><td>Path to store certificates, challenges etc</td><td>"/etc/proksi/lets_encrypt"</td></tr><tr><td><code>host</code></td><td>The hosts proksi accepts requests for, by name (matched against the <code>Host</code> header)</td><td></td></tr><tr><td><code>host.&lt;name&gt;.tls</code></td><td>Certificate files (<code>certificate.pem</code>, <code>certificate.key</code>) and <code>self_signed_on_failure</code></td><td></td></tr><tr><td><code>host.&lt;name&gt;.headers.response</code></td><td>Headers added (<code>add</code>) or removed (<code>remove</code>) in every response of the host</td><td></td></tr><tr><td><code>host.&lt;name&gt;.location.&lt;path&gt;</code></td><td>Requests whose path is <code>&lt;path&gt;</code> or below it; the most specific location wins</td><td></td></tr><tr><td><code>host.&lt;name&gt;.location.&lt;path&gt;.upstream.&lt;host:port&gt;</code></td><td>Where requests are sent (load balanced), with optional <code>weight</code>, <code>network</code>, <code>sni</code> and request <code>headers.add</code></td><td></td></tr><tr><td></td><td></td><td></td></tr></tbody></table>



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

# The configuration for the HTTPS & HTTP service.
server:
  # The address that the server will listen on while serving HTTPS.
  # This can be a TCP address or a Unix socket.
  # The default value is "0.0.0.0:443".
  https_address: "0.0.0.0:443"

  # The address that the server will listen on for HTTP requests.
  # This can be a TCP address or a Unix socket.
  # The default value is "0.0.0.0:80".
  http_address: "0.0.0.0:80"

  # Set to false to only serve plain HTTP (every host then needs `tls: false`).
  # The default value is true.
  https_enabled: true


# The configuration for the Let's Encrypt integration.
lets_encrypt:

  # Whether the Let's Encrypt integration is enabled
  # (the background service will run and issue certificates for your routes).
  # Note: You must explicitly enable this and provide a valid email address
  enabled: false

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


# Hosts that the server will accept requests for, by name.
# See the HCL page for how locations are matched.
host:
  example.com:
    # Headers added or removed in every response of the host
    headers:
      response:
        add:
          X-Api-Version: "1.0"
        remove:
          - "Server"

    location:
      # Requests to /api and below
      /api:
        upstream:
          "10.1.2.24:3000":
            # The docker network to reach the upstream through (optional)
            network: "public"
          "10.1.2.23:3000":
            network: "shared"

      # Everything else
      /:
        upstream:
          "10.1.2.25:3000": {}
```
