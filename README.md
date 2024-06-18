# Proksi: Automatic SSL, HTTP, and DNS Proxy

![GitHub Release](https://img.shields.io/github/v/release/luizfonseca/proksi?style=for-the-badge)
![Crates.io MSRV](https://img.shields.io/crates/msrv/proksi?style=for-the-badge)
![Crates.io License](https://img.shields.io/crates/l/proksi?style=for-the-badge)
![Crates.io Total Downloads](https://img.shields.io/crates/d/proksi?style=for-the-badge)
<img src="./assets/crabbylogoproksi.png" alt="logo" width="100" style="float: left;"/>

<div>
<a href="https://discord.gg/Auw36pPSXf" target="_blank"><img src="./assets/discord.png" alt="discord-logo" width="200" style="float: left;"/></a>

</div>


> ⚠️ Important: this is still a work-in-progress project.
> It does the basics but everything needs polishing and real production testing.
> That said, suggestions, issues or PRs are encouraged.

Proksi is a simple, lightweight, and easy-to-use proxy server that automatically handles SSL, HTTP, and DNS traffic. It is designed to be used as a standalone proxy server or as a component in a larger system. Proksi is written in Rust and uses Pingora as its core networking library.


- [Proksi: Automatic SSL, HTTP, and DNS Proxy](#proksi-automatic-ssl-http-and-dns-proxy)
  - [Features](#features)
  - [Installation](#installation)
    - [Docker](#docker)
    - [Binary](#binary)
    - [Rust Cargo](#rust-cargo)
  - [Configuration](#configuration)
    - [Environment variables](#environment-variables)
    - [HCL (recommended)](#hcl-recommended)
      - [HCL functions](#hcl-functions)
    - [YAML](#yaml)
  - [Documentation](#documentation)
  - [Docker](#docker-1)
    - [Docker Labels](#docker-labels)
      - [Docker Swarm](#docker-swarm)
    - [Docker container](#docker-container)
  - [Batteries included](#batteries-included)
  - [Performance \& Benchmarks](#performance--benchmarks)
  - [Why build another proxy...?](#why-build-another-proxy)


## Features


- [x] **HTTPS**: Automatic Redirect, SSL termination and certificate renewal (Let's Encrypt) 
- [x] **Docker**: Swarm/Compose Label Support
- [x] **Load Balancing** (✅ Round Robin, ⛔︎ Weighted Round Robin, ⛔︎ Least Connections)
- [x] **HCL functions** Extensible through configuration with `env` and `import`
- [x] Path matcher (regex, prefix and suffix)~ Pattern-based for high performance and flexibility
- [x] Header manipulation for DOWNSTREAM & UPSTREAM (add/replace, remove)
- [X] Health Checks
- [X] **Basic** Authentication
- [X] **Oauth2** Authentication (Google, Facebook, ✅ Github, ✅ WorkOs etc)
- [X] **RequestId** Middleware

## Installation

### Docker

Similar to other proxies, Proksi can be run as a Docker container. The following command will run the proxy server on ports 80 and 443, and mount the configuration file from the host machine to the container:

```bash
mkdir config
touch config/proksi.hcl
docker run -d -p 80:80 -p 443:443 -v ./config:/etc/proksi/ luizfonseca/proksi:latest
```

### Binary

You can also run Proksi by downloading a binary from the [Github Releases page](https://github.com/luizfonseca/proksi/releases) for you platform. For example, for Ubuntu:

```bash
# Download the binary (change {VERSION} to the one you want)
wget https://github.com/luizfonseca/proksi/releases/download/{VERSION}/proksi-linux-gnu-x86_64.tar.gz
tar -xvf proksi-linux-gnu-x86_64.tar.gz
chmod +x proksi
./proksi
```

### Rust Cargo

You can also run Proksi using `cargo`:

```bash
cargo install proksi
mkdir config
touch config/proksi.hcl 
proksi -c ./ --service_name=my_proxy
```


## Configuration

Proksi can be configured through HCL, YAML or Environment variables.
See [the examples folder](./examples) to learn more about how to use Proksi using HCL or YAML.


### Environment variables

Proksi can be configured using environment variables. They are mapped to the configuration file,  always start with `PROKSI_` and can be used to override the default values.

Example:
- For the key `worker_threads`, the environment variable `PROKSI_WORKER_THREADS` can be used
- For the key `logging.level`, the environment variable `PROKSI_LOGGING__LEVEL` can be used (note the `__` separator due to the nested key)
- For keys that accept a list of values, e.g. `routes`, the environment variable `PROKSI_ROUTES` can be used with a string value like this:

```bash
export PROKSI_ROUTES='[{host="example.com", upstreams=[{ip="10.0.1.24", port=3001}]'
```


### HCL (recommended)

Proksi can be configured using HCL (HashiCorp Configuration Language). This is the recommended way to configure Proksi, as it is more human-readable and easier to work with than JSON or YAML as well as it offers `functions` that you can use throughout your configuration:

```bash
touch proksi.hcl
```

```hcl
worker_threads = env("WORKER_THREADS")

lets_encrypt {
  enabled = true
  email = env("LETS_ENCRYPT_EMAIL")
  staging = true
}

paths {
  lets_encrypt = env("LETS_ENCRYPT_PATH")
}

// You can split your websites into separate files
routes = [
  import("./sites/mywebsite.com.hcl"),
  import("./sites/myotherwebsite.co.uk.hcl")
]

// Or you can define them here
routes = [
  {
    host = "cdn.example.com"
    ssl_certificate {
      // Useful for development
      self_signed_on_failure = true
    }
    upstreams {
      ip = "example.com"
      port = 443

      headers {
        add = [
          { name = "Host", value = "example.com" }, 
          { name = "X-Proxy-For", value = "cdn.example.com" }
        ]
      }
    }
  }
]
```

#### HCL functions

HCL supports functions that can be used to generate values in the configuration file. The following functions are supported:

- `env(name: string)`: Get the value of an environment variable. If the environment variable is not set, an error is thrown.
- `import(path: string)`: Import another HCL file. The path is relative to the current file.

### YAML

You can see below an excerpt of the configuration . This is still a work in progress, and the configuration format may change in the future.

```yaml
# Example configuration file
worker_threads: 4

logging:
  level: INFO

lets_encrypt:
  enabled: true
  # This issues temporary certificates for testing. Flip it to `false` to use
  # production certificates.
  staging: true
  email: "your-email@example.com"

paths:
  # where to store certificates?
  lets_encrypt: "./my-lets_encrypt-folder"

routes:
  - host: "example.com"
    ssl_certificate:
      # Useful for testing only
      self_signed_on_failure: true
    upstreams:
      - ip: "10.1.2.24/24"
        port: 3000
        network: "public"
```


## Documentation
For more tutorials, guides, and extended documentation, please refer to the https://docs.proksi.info.




## Docker

### Docker Labels
Proksi can be used in conjunction with Docker to automatically discover **services** and route traffic to them. To do this, you need to add labels to your Docker services (*swarm* or not).


#### Docker Swarm

```yaml
# docker-compose.yml
# This is an example of how to use Proksi with Docker containers
# This will automatically discover services and route traffic to them
# based on the labels defined in the container.

networks:
  web:
    name: web

services:
  # Proksi itself -- the only service that needs the `ports` directive
  proksi:
    environment:
      PROKSI_LOGGING__LEVEL: "info"
      PROKSI_WORKER_THREADS: 4

      PROKSI_DOCKER__ENABLED: "true"
      PROKSI_DOCKER__MODE: "swarm"

      PROKSI_LETS_ENCRYPT__ENABLED: "true"
      PROKSI_LETS_ENCRYPT__STAGING: "true"
      PROKSI_LETS_ENCRYPT__EMAIL: "contact@your-website.com"

      PROKSI_PATHS__LETS_ENCRYPT: "/etc/proksi/certs"
    image: luizfonseca/proksi:latest
    networks:
      - web # Proksi needs to be in the same network as the services it will proxy
    ports:
      - "80:80"
      - "443:443"
    volumes:
      # "data" folder should exist
      - ./data:/etc/proksi/certs

  # Your service
  # This service will be automatically discovered by Proksi and doesn't need
  # to expose any ports to the host, only to proksi
  web:
    image: nginxdemos/hello # This container exposes port 80
    networks:
      - web
    deploy:
      labels:
        proksi.enabled: "true"
        proksi.host: "myhost.example.com"
        proksi.port: "80"

        # you can make Proksi to use a self-signed certificate (in-memory)
        proksi.ssl_certificate.self_signed_on_failure: "true"

        # (Optional)
        # E.g. `/api` will match only requests with "example.com/api*" to this service.
        proksi.path.pattern.api: "/api"

        # (Optional) Plugins
        proksi.plugin.request_id.enabled: "true"

        proksi.plugins.oauth2.provider: github
        proksi.plugins.oauth2.client_id: <client_id>
        proksi.plugins.oauth2.client_secret: <client_secret>
        proksi.plugins.oauth2.jwt_secret: <jwt_secret> # The secret used to sign the JWT token
        proksi.plugins.oauth2.validations:  |
          [ { "type": "email", "value": ["your-email@example.com"] } ]
```

### Docker container

It's a very similar setup to the Docker Swarm, but the `labels` are defined outside of the `deploy` key and `PROKSI_DOCKER__MODE` is set to `container`:

```yaml
services:
  # proksi: ...

  # Your service
  # This service will be automatically discovered by Proksi and doesn't need
  # to expose any ports to the host, only to proksi
  web:
    image: nginxdemos/hello # This container exposes port 80
    networks:
      - web
    labels:
      proksi.enabled: "true"
      proksi.host: "myhost.example.com"
      proksi.port: "80"

      # ... other labels
```

## Batteries included

Proksi is designed to be a standalone proxy server that can be used out of the box. It comes with a series of features that would normally require multiple tools to achieve. The vision of this project is to also cover most of the basic and extended requirements for a LB/Proxy server without getting into "convoluted" configurations.

The following features are included or will be included into Proksi without the need of 3rd party plugins:

- [X] **Basic** Authentication
- [X] **Oauth2** Authentication (Google, Facebook, ✅ Github, ✅ WorkOs etc)
- [X] **RequestId** Middleware
- [ ] **Rate Limiting** Middleware
- [ ] **Geofence**
- [ ] **IP range** blocking
- [ ] **IP allowlists/denylists**
- [ ] Others


Note that as always, these are mostly opt-in and are **disabled by default**.


## Performance & Benchmarks

Early tests are promising, but we need to do more testing to see how Proksi performs under *real* load. There are also some optimizations that can be done to improve performance in the long term, though the focus is on making it feature complete first.

An sample run from the `wrk` benchmark on the simple `/ping` endpoint shows the following results (running on a **single** worker thread):

```bash
# Apple M1 Pro, 16GB
# Memory Usage: 15.2MB, CPU Usage: 13%, 1 worker thread
# at 2024-06-13
# Wrk > 50 connections, 4 threads, 30s duration
wrk -c 50 -t 4 -d 30s http://127.0.0.1/ping

Running 30s test @ http://127.0.0.1/ping
  4 threads and 50 connections
  Thread Stats   Avg      Stdev     Max   +/- Stdev
    Latency   376.73us  124.78us   7.83ms   91.64%
    Req/Sec    31.76k     3.03k   34.29k    92.44%
  Latency Distribution
     50%  373.00us
     75%  404.00us
     90%  442.00us
     99%  675.00us
  3803987 requests in 30.10s, 467.98MB read
Requests/sec: 126377.09
Transfer/sec:     15.55MB
```

It's also based on [Pingora](https://github.com/cloudflare/pingora), so it should be fast if cloudflare is using it.


## Why build another proxy...?

Many reasons, but the main one is that I wanted to learn more about how proxies work, and I wanted to build something that I could use in my own projects. I also wanted to build something that was easy to use and configure, and that could be extended with custom plugins AND offered out-of-the-box
things that other projects needed the community to make.

Also, Rust is a very good use case for proxies (lower CPU usage, lower memory usage etc) and Cloudflare Pingora is basically a mold that you can create things with.

This project will be used in my personal experiments and I welcome you to explore and contribute as you wish.
