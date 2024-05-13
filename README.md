# Proksi: Automatic SSL, HTTP, and DNS Proxy


Proksi is a simple, lightweight, and easy-to-use proxy server that automatically handles SSL, HTTP, and DNS traffic. It is designed to be used as a standalone proxy server or as a component in a larger system. Proksi is written in Rust and uses Pingora as its core networking library.

## Usage

### Docker

Similar to other proxies, Proksi can be run as a Docker container. The following command will run Proksi in a Docker container:

```bash
docker run -d -p 80:80 -p 443:443 -v /path/to/config:/etc/proksi/config.yaml luizfonseca/proksi
```

### Binary

You can also run Proksi as a standalone binary. First, you need to build the binary:

```bash
cargo build --release
```

Then you can run the binary in your platform:

```bash
./target/release/proksi
```

## Running Proksi

### Docker Labels

Proksi can be used in conjunction with Docker to automatically discover services and route traffic to them. To do this, you need to add labels to your Docker containers. The following labels are supported:

- `proksi.enabled`: Whether the service should be proxied or not. By default, Proksi won't discover any services where the value is `false`.
- `proksi.host`: The hostname that the service should be available at. E.g. `example.com`.
- `proksi.path_prefix`: The path that the service should be available at. E.g. `/api`.
- `proksi.path.suffix`: The suffix that the service will use to handle requests. E.g. `.json`.
- `proksi.headers.add`: An object containing headers to add to the request. Each header should have a `name` and a `value`. E.g. `[{name="X-Forwarded-For", value="my-api"}, {name="X-Api-Version", value="1.0"}]`.
- `proksi.headers.remove`: A list of comma-separated headers to remove from the request at the end of proxying. E.g. `Server,X-User-Id`.
- `proksi.port`: The port that the current service is running on.



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
- [ ] Automatic SSL termination (using LetsEncrypt)
- [ ] Automatic HTTP to HTTPS redirection
- [ ] Docker Labeling Support (for services discovery)
- [ ] Load Balancing (Round Robin, Weighted Round Robin, Least Connections)
- [ ] Health Checks
- [ ] Storage support for LetsEncrypt certificates (S3, Etcd, Consul, etc)
- [ ] Controller <> server support (in order to share certificates)

### Middlewares/Plugins
- [ ] **Geofence**
- [ ] **IP range** blocking
- [ ] **IP allowlists/denylists**
- [ ] **Basic** Authentication
- [ ] **Oauth2** Authentication (Google, Facebook, Github, etc)
- [ ] **JWT** Authentication (and thus, passing information downstream)
- [ ] **CORS** Middleware
- [ ] **Rate Limiting** Middleware
- [ ] **Rewrite** Middleware
- [ ] **Redirect** Middleware
- [ ] **Compression** Middleware
- [ ] **Request/Response Logging** Middleware
- [ ] **Request/Response Modification** Middleware
- [ ] **Request/Response Caching** Middleware
- [ ] **Request/Response Filtering** Middleware
- [ ] **Request/Response Transformation** Middleware
- [ ] **Request/Response Validation** Middleware
- [ ] **RequestId** Middleware


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
