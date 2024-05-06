# Proksi: Automatic SSL, HTTP, and DNS Proxy


Proksi is a simple, lightweight, and easy-to-use proxy server that automatically handles SSL, HTTP, and DNS traffic. It is designed to be used as a standalone proxy server or as a component in a larger system. Proksi is written in Rust and uses Pingora as its core networking library.

## Features

- [x] Automatic Redirect to HTTPS
- [ ] Docker Labeling Support
- [ ] Automatic SSL termination
- [ ] Automatic SSL certificate renewal
- [ ] Extensible through configuration


## Configuration

You can see below an excerpt of the configuration (generated from Cue). This is still a work in progress, and the configuration format may change in the future.

```yaml
proksi:
  general:
    ports:
      http: 8080
      https: 8443
    host: localhost
    logLevel: info
  middlewares:
    - name: cors
      options:
        origin: '*'
  routes:
    - host: http://example.com
      upstreams:
        - addr: 10.0.1.24/24
          port: 3000
          weight: 2
        - addr: 10.0.1.26/24
          port: 3000
          weight: 1
      paths:
        - path: /api
      middlewares:
        - name: cors
          options:
            origin: '*'

```

## Performance & Benchmarks

TBA. It's based on [Pingora](https://github.com/cloudflare/pingora), so it should be fast if cloudflare is using it.
