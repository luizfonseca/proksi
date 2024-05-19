# Proksi: Automatic SSL, HTTP, and DNS Proxy

![GitHub Release](https://img.shields.io/github/v/release/luizfonseca/proksi?style=for-the-badge)
![Crates.io MSRV](https://img.shields.io/crates/msrv/proksi?style=for-the-badge)
![Crates.io License](https://img.shields.io/crates/l/proksi?style=for-the-badge)
![Crates.io Total Downloads](https://img.shields.io/crates/d/proksi?style=for-the-badge)


![logo](./assets/crabbylogoproksi.png)


> ⚠️ Important: this is still a work-in-progress project.
> It does the basics but everything needs polishing and real production testing.
> That said, suggestions, issues or PRs are encouraged.

Proksi is a simple, lightweight, and easy-to-use proxy server that automatically handles SSL, HTTP, and DNS traffic. It is designed to be used as a standalone proxy server or as a component in a larger system. Proksi is written in Rust and uses Pingora as its core networking library.


- [Proksi: Automatic SSL, HTTP, and DNS Proxy](#proksi-automatic-ssl-http-and-dns-proxy)
  - [Getting started](#getting-started)
  - [Usage](#usage)
    - [Docker](#docker)
    - [Docker Swarm](#docker-swarm)
    - [Binary](#binary)
    - [Command line options](#command-line-options)
  - [Running Proksi](#running-proksi)
    - [Docker Labels](#docker-labels)
  - [Jobs to be done](#jobs-to-be-done)
  - [Batteries included](#batteries-included)
    - [Proxy](#proxy)
    - [Middlewares/Plugins](#middlewaresplugins)
    - [Extending Proksi](#extending-proksi)
  - [Configuration](#configuration)
    - [YAML/TOML Configuration](#yamltoml-configuration)
    - [Environment variables](#environment-variables)
  - [Configuration Examples](#configuration-examples)
  - [Performance \& Benchmarks](#performance--benchmarks)
  - [Why build another proxy...?](#why-build-another-proxy)

## Getting started
[![asciicast](https://asciinema.org/a/ORhG5Na2SHIBI8TH2mPPUHMVZ.svg)](https://asciinema.org/a/ORhG5Na2SHIBI8TH2mPPUHMVZ)


## Usage

### Docker

Similar to other proxies, Proksi can be run as a Docker container. The following command will run Proksi in a Docker container:

```bash
docker run -d -p 80:80 -p 443:443 -v /path/to/config:/etc/proksi/ luizfonseca/proksi
```

### Docker Swarm
One of the main purposes of Proksi is to also enable automatic service discovery and routing. To do this, you can use Proksi in conjunction with Docker Swarm:

```yaml
# docker-compose.yml
# This is an example of how to use Proksi with Docker Swarm
# This will automatically discover services and route traffic to them
# based on the labels defined in the service.

version: '3.8'
services:
  proksi:
    image: luizfonseca/proksi:latest
    network:
      - web # Any service in the same network will be able to communicate with Proksi
    ports:
      - "80:80"
      - "443:443"
    volumes:
      - /path/to/config:/etc/proksi/config.yaml
    deploy:
      placement:
        constraints:
          - node.role == manager
```


### Binary

You can also run Proksi as a standalone binary using rust's `cargo`.
First, you need to install the binary:

```bash
cargo install proksi
```

Then you can run the binary in your platform:

```bash
# Proksi will try to find proksi.yaml or proksi.toml in this path
proksi -c /config-path/ --service_name=proksi
```

### Command line options
Running `proksi --help` will provide you with the available options.

```bash
Usage: proksi [OPTIONS]
Options:
  -s, --service-name <SERVICE_NAME>
          The name of the service (will appear as a log property)

          [default: proksi]

  -w, --worker-threads <WORKER_THREADS>
          The number of worker threads to be used by the HTTPS proxy service.

          For background services the default is always (1) and cannot be changed.

          [default: 1]

  -c, --config-path <CONFIG_PATH>
          The PATH to the configuration file to be used.

          The configuration file should be named either proksi.toml, proksi.yaml or proksi.yml

          and be present in that path. Defaults to the current directory.

          [default: ./]

      --level <LEVEL>
          The level of logging to be used

          [default: info]
          [possible values: debug, info, warn, error]

      --access-logs-enabled
          Whether to log access logs (request, duration, headers etc)

      --error-logs-enabled
          Whether to log error logs (errors, panics, etc) from the Rust runtime

  -h, --help
          Print help (see a summary with '-h')

  -V, --version
          Print version
```


## Running Proksi

### Docker Labels

Proksi can be used in conjunction with Docker to automatically discover **services** and route traffic to them. To do this, you need to add labels to your Docker services (*swarm* or not).
The following labels are supported:

```yaml
# docker-compose.yml
version: 3.8
services:
  my_service:
    image: your-service:latest
    ports:
      - "3000:3000"
    deploy:
      labels:
        # Whether the service should be proxied or not.
        # By default, Proksi won't discover any services where the value is not explicitly `true`
        proksi.enable: "true"

        # The hostname that the service should be available at. E.g. `example.com`.
        proksi.host: "example.com"

        # The port that your service is running on. E.g. `3000`.
        proksi.port: "3000"

        # (Optional) The path prefix that the service should be available at.
        # E.g. `/api` will match only requests with "example.com/api*" to this service.
        proksi.path.prefix: "/api"

        # (Optional) The suffix that the service will use to handle requests.
        # E.g. `.json` will match only requests with "example.com/*.json"
        proksi.path.suffix: ".json"

        # (Optional) A dictionary of Headers to add to the response at the end of proxying
        proksi.headers.add: |
          [
            {name="X-Forwarded-For", value="my-api"},
            {name="X-Api-Version", value="1.0\"}
          ]

        # A list of comma-separated headers to remove from the response at the end of proxying.
        proksi.headers.remove: "Server,X-User-Id"
```

## Jobs to be done

- [x] Automatic Redirect to HTTPS
- [ ] Docker Labeling Support
- [x] Automatic SSL termination
- [x] Automatic SSL certificate renewal
- [x] Extensible through configuration
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
- [X] Automatic SSL termination (using LetsEncrypt)
- [X] Automatic HTTP to HTTPS redirection
- [ ] Docker Labeling Support (for services discovery)
- [ ] Load Balancing (Round Robin, Weighted Round Robin, Least Connections)
- [X] Health Checks
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


### YAML/TOML Configuration
You can see below an excerpt of the configuration (generated from Cue). This is still a work in progress, and the configuration format may change in the future.

```yaml
# Example configuration file
service_name: "proksi"
worker_threads: 4
logging:
  level: "INFO"
  access_logs_enabled: true
  error_logs_enabled: false
lets_encrypt:
  enabled: true
  # This issues temporary certificates for testing. Flip it to `false` to use
  # production certificates.
  staging: true
  email: "your-email@example.com"
paths:
  lets_encrypt: "./my-lets_encrypt-folder"
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


### Environment variables

Proksi can be configured using environment variables. They are mapped to the configuration file,  always start with `PROKSI_` and can be used to override the default values.
For nested keys, use the `__` character.

Example:
- For the key `service_name`, the environment variable `PROKSI_SERVICE_NAME` can used
- For the key `worker_threads`, the environment variable `PROKSI_WORKER_THREADS` can be used
- For the key `logging.level`, the environment variable `PROKSI_LOGGING__LEVEL` can be used (note the `__` separator due to the nested key)
- For keys that accept a list of values, e.g. `routes`, the environment variable `PROKSI_ROUTES` can be used with a string value like this:

```bash
export PROKSI_ROUTES='[{host="example.com", upstreams=[{ip="10.0.1.24", port=3001}]'
```

In the future you might be able to use `PROKSI_ROUTES__0__HOST` to set the host of the first route (or any other), but this is not yet implemented.


## Configuration Examples
See [the examples folder](./examples) to learn about how to use Proksi.


## Performance & Benchmarks

Early tests are promising, but we need to do more testing to see how Proksi performs under *real* load. There are also some optimizations that can be done to improve performance in the long term, though the focus is on making it feature complete first.

An sample run from the `wrk` benchmark on the simple `/ping` endpoint shows the following results:

```bash
# Apple M1 Pro, 16GB
# Memory Usage: 15MB, CPU Usage: 13%, 4 worker threads
# at 2024-05-16T23:47
# Wrk > 50 connections, 4 threads, 30s duration
wrk -c 50 -t 4 -d 30s http://127.0.0.1/ping

Running 30s test @ http://127.0.0.1/ping
  4 threads and 50 connections
  Thread Stats   Avg      Stdev     Max   +/- Stdev
    Latency   561.57us  218.11us  14.91ms   84.14%
    Req/Sec    21.43k     3.53k   24.72k    79.32%
  2566858 requests in 30.10s, 315.79MB read
Requests/sec:  85274.55
Transfer/sec:     10.49MB
```

It's also based on [Pingora](https://github.com/cloudflare/pingora), so it should be fast if cloudflare is using it.


## Why build another proxy...?

Many reasons, but the main one is that I wanted to learn more about how proxies work, and I wanted to build something that I could use in my own projects. I also wanted to build something that was easy to use and configure, and that could be extended with custom plugins AND offered out-of-the-box
things that other projects needed the community to make.

Also, Rust is a very good use case for proxies (lower CPU usage, lower memory usage etc) and Cloudflare Pingora is basically a mold that you can create things with.

This project will be used in my personal experiments and I welcome you to explore and contribute as you wish.
