# Proksi: Automatic SSL, HTTP, and DNS Proxy


Proksi is a simple, lightweight, and easy-to-use proxy server that automatically handles SSL, HTTP, and DNS traffic. It is designed to be used as a standalone proxy server or as a component in a larger system. Proksi is written in Rust and uses Pingora as its core networking library.

## Jobs to be done

- [x] Automatic Redirect to HTTPS
- [ ] Docker Labeling Support
- [X] Automatic SSL termination
- [ ] Automatic SSL certificate renewal
- [X] Extensible through configuration
- [ ] Path matcher (regex, prefix and suffix)
- [ ] Default middlewares implemented
  - [ ] RateLimiter,
  - [ ] GeoIp/Ip whitelisting
  - [ ] BasicAuth
  - [ ] Oauth2
  - [ ] CORS


## Batteries included

Proksi is designed to be a standalone proxy server that can be used out of the box. It comes with a series of features that would normally require multiple tools to achieve. The vision of this project is to also cover most of the basic and extended requirements for a LB/Proxy server without getting into "convoluted" configurations.

The following features are included or will be included into Proksi without the need of 3rd party plugins:

### Proxy
- [] Automatic SSL termination (using LetsEncrypt)
- [] Automatic HTTP to HTTPS redirection
- [] Docker Labeling Support (for services discovery)
- [] Load Balancing (Round Robin, Weighted Round Robin, Least Connections)
- [] Health Checks
- [] Storage support for LetsEncrypt certificates (S3, Etcd, Consul, etc)
- [] Controller <> server support (in order to share certificates)

### Middlewares/Plugins
- [] **Geofence**
- [] **IP range** blocking
- [] **IP allowlists/denylists**
- [] **Basic** Authentication
- [] **Oauth2** Authentication (Google, Facebook, Github, etc)
- [] **JWT** Authentication (and thus, passing information downstream)
- [] **CORS** Middleware
- [] **Rate Limiting** Middleware
- [] **Rewrite** Middleware
- [] **Redirect** Middleware
- [] **Compression** Middleware
- [] **Request/Response Logging** Middleware
- [] **Request/Response Modification** Middleware
- [] **Request/Response Caching** Middleware
- [] **Request/Response Filtering** Middleware
- [] **Request/Response Transformation** Middleware
- [] **Request/Response Validation** Middleware
- [] **RequestId** Middleware


Note that as always, these are mostly opt-in and will be **disabled by default**.

### Extending Proksi

Proksi is designed to be extensible through configuration. You can define custom middlewares and plugins in the configuration file, and Proksi will automatically load and use them. This allows you to add new features to Proksi without having to modify the source code.

The plugins support will be aimed towards **WASM**, so you can write your own plugins in any language that compiles to **WASM**.

If you have any ideas for new features or plugins, that could benefit the project, please open an issue or a PR.

## Configuration

You can see below an excerpt of the configuration (generated from Cue). This is still a work in progress, and the configuration format may change in the future.

```yaml
# Example configuration file
service_name: "proksi"
logging:
  level: "INFO"
  access_logs: true
  error_logs: false
paths:
  tls_certificates: "/etc/proksi/certificates"
  tls_challenges: "/etc/proksi/challenges"
  tls_order: "/etc/proksi/orders"
  tls_account_credentials: "/etc/proksi/account-credentials"
routes:
  - host: "example.com"
    path_prefix: "/api"
    headers:
      add:
        - name: "X-Forwarded-For"
          value: "<value>"
        - name: "X-Api-Version"
          value: "1.0"
      remove:
        - name: "Server"
    upstreams:
      - ip: "10.1.2.24/24"
        port: 3000
        network: "public"
      - ip: "10.1.2.23/24"
        port: 3000
        network: "shared"
```

## Examples
See (the examples folder)[./examples] to learn about how to use Proksi.


## Performance & Benchmarks

TBA.

It's based on [Pingora](https://github.com/cloudflare/pingora), so it should be fast if cloudflare is using it.
