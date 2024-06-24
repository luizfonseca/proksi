# TOML

You can check below an example TOML file based on the previous YAML file.

```toml
# Description: Example configuration file for Proksi
#
# Proksi is a reverse proxy server that can be used to route 
# incoming requests to different upstream servers based on the request's host, path, 
# headers, and other attributes.
#
# This configuration file specifies the following settings:
#
#
# ------------------------------------------------------------------
# The name of the service is "proksi".
# This will show in logs and it's mostly used for log filtering if needed
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

# The configuration for the Let's Encrypt integration.
[lets_encrypt]
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

# The logging configuration for the server.
[logging]
# The log level for the server (can be "DEBUG", "INFO", "WARN", "ERROR").
level = "INFO"

# Whether access logs are enabled.
# When set to <true>, the server will log information about incoming requests.
# This information includes the request method, path, status code, response time and more.
access_logs_enabled = true

# Whether error logs are enabled.
error_logs_enabled = false

# The paths for the TLS certificates, challenges, orders, and account credentials.
# You can override any, these are the current defaults.
[paths]
# The path where the TLS certificates will be stored.
lets_encrypt = "/etc/proksi/certificates"

# The list of routes that the server will use to route incoming requests
# to different upstream servers.
# Each route is an item in the list and it has the following attributes:
[[routes]]

# The host attribute specifies the hostname that the route will match.
# This is normally the domain, subdomain that you want to route to a particular server/ip.
# This can be a domain name or an IP address. For IP address, no certificate will be issued.
# The host attribute is required.
host = "example.com"

# The path_prefix attribute specifies the path prefix that the route will match.
path_prefix = "/api"

# The headers attribute specifies the headers that will
# be added or removed at the *end* of the response
# --
# In the near future you will be able to modify the headers
# of the request send to the upstream server
# (Adds) the given headers to the dowstream (client) response
#
[[routes.headers.add]]
name = "X-Forwarded-For"
value = "<value>"

[[routes.headers.add]]
name = "X-Api-Version"
value = "1.0"

# Removes the given headers from the dowstream (client) response
[[routes.headers.remove]]
name = "Server"

# The upstreams attribute specifies the list of upstream servers that the route will use.
# These are load balanced and the server will try to connect to the first one in the list.
# If the connection fails, it will try the next one.
# --
# Health checks run in the background to ensure you have a healthy connection always.
[[routes.upstreams]]

# The IP address of the upstream server
# (can be any IP address, as long as Proksi can access it).
ip = "10.1.2.24/24"
# The port of the upstream server (can be any port).
port = 3_000
# The network attribute specifies the network that the upstream server is part of.
# This is mostly important for Docker containers, but it can be used for other purposes.
network = "public"

[[routes.upstreams]]
ip = "10.1.2.23/24"
port = 3_000
network = "shared"

```
