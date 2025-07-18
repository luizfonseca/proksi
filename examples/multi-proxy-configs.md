# Multi-Proxy Configuration Examples

This document provides comprehensive examples of the new multi-proxy architecture introduced in proksi.

## Basic Multi-Proxy Setup

### YAML Configuration
```yaml
service_name: "multi-proxy-server"

# New multi-proxy format
proxies:
  # HTTPS proxy with SSL termination
  - host: "0.0.0.0:443"
    ssl:
      enabled: true
      min_proto: "v1.2"
      max_proto: "v1.3"
      acme:
        enabled: true
        challenge_port: 80
    routes:
      - host: "secure.example.com"
        upstreams:
          - ip: "10.0.1.1"
            port: 3000
      - host: "api.example.com"
        upstreams:
          - ip: "10.0.1.2" 
            port: 8080
    worker_threads: 4

  # HTTP-only proxy (no SSL)
  - host: "0.0.0.0:8080"
    ssl: null  # No SSL configuration
    routes:
      - host: "internal.example.com"
        upstreams:
          - ip: "10.0.2.1"
            port: 3000
    worker_threads: 2

logging:
  level: "info"
  format: "json"
  
lets_encrypt:
  email: "admin@example.com"
  staging: false
```

### HCL Configuration
```hcl
service_name = "multi-proxy-server"

proxies = [
  {
    host = "0.0.0.0:443"
    ssl = {
      enabled = true
      min_proto = "v1.2"
      max_proto = "v1.3"
      acme = {
        enabled = true
        challenge_port = 80
      }
    }
    routes = [
      {
        host = "secure.example.com"
        upstreams = [
          { ip = "10.0.1.1", port = 3000 }
        ]
      },
      {
        host = "api.example.com"
        upstreams = [
          { ip = "10.0.1.2", port = 8080 }
        ]
      }
    ]
    worker_threads = 4
  },
  {
    host = "0.0.0.0:8080"
    ssl = null
    routes = [
      {
        host = "internal.example.com"
        upstreams = [
          { ip = "10.0.2.1", port = 3000 }
        ]
      }
    ]
    worker_threads = 2
  }
]

logging = {
  level = "info"
  format = "json"
}

lets_encrypt = {
  email = "admin@example.com"
  staging = false
}
```

## Cloudflare Containers + Direct SSL Example

Perfect for environments where some traffic comes through Cloudflare (no SSL needed) and some comes directly (SSL required).

```yaml
service_name: "cloudflare-mixed-setup"

proxies:
  # Cloudflare-proxied traffic (HTTP only, SSL handled by CF)
  - host: "0.0.0.0:8080"
    ssl: null
    routes:
      - host: "cf-proxied.example.com"
        upstreams:
          - ip: "10.0.1.1"
            port: 3000
        headers:
          add:
            - name: "X-Forwarded-Proto"
              value: "https"
    worker_threads: 2

  # Direct traffic (HTTPS with Let's Encrypt)  
  - host: "0.0.0.0:443"
    ssl:
      enabled: true
      acme:
        enabled: true
        challenge_port: 80
    routes:
      - host: "direct.example.com"
        upstreams:
          - ip: "10.0.1.2"
            port: 3000
    worker_threads: 4
```

## Development vs Production Environments

```yaml
service_name: "dev-prod-mixed"

proxies:
  # Production HTTPS (port 443)
  - host: "0.0.0.0:443"
    ssl:
      enabled: true
      acme:
        enabled: true
    routes:
      - host: "prod.example.com"
        upstreams:
          - ip: "10.0.1.100"
            port: 3000
    worker_threads: 4

  # Development HTTP (port 8080)
  - host: "0.0.0.0:8080"
    ssl: null
    routes:
      - host: "dev.example.com"
        upstreams:
          - ip: "10.0.1.200"
            port: 3000
        headers:
          add:
            - name: "X-Environment"
              value: "development"
    worker_threads: 1

  # Staging HTTPS with custom ACME port (port 8443)
  - host: "0.0.0.0:8443"
    ssl:
      enabled: true
      acme:
        enabled: true
        challenge_port: 8081
    routes:
      - host: "staging.example.com"
        upstreams:
          - ip: "10.0.1.300"
            port: 3000
    worker_threads: 2
```

## Load Balancer Behind Load Balancer

```yaml
service_name: "lb-behind-lb"

proxies:
  # External load balancer frontend (HTTP only)
  - host: "0.0.0.0:80"
    ssl: null
    routes:
      - host: "external.example.com"
        upstreams:
          - ip: "10.0.1.1"
            port: 3000
            weight: 3
          - ip: "10.0.1.2" 
            port: 3000
            weight: 2
          - ip: "10.0.1.3"
            port: 3000
            weight: 1
    worker_threads: 8

  # Internal services (HTTPS with mutual TLS)
  - host: "0.0.0.0:8443"
    ssl:
      enabled: true
      min_proto: "v1.3"  # Require TLS 1.3
      acme:
        enabled: false  # Custom certificates
    routes:
      - host: "internal-api.example.com"
        ssl:
          path:
            key: "/etc/proksi/certs/internal.key"
            pem: "/etc/proksi/certs/internal.pem"
        upstreams:
          - ip: "10.0.2.1"
            port: 8080
    worker_threads: 2
```

## Microservices Architecture

```yaml
service_name: "microservices-gateway"

proxies:
  # Public API Gateway (HTTPS)
  - host: "0.0.0.0:443"
    ssl:
      enabled: true
      acme:
        enabled: true
    routes:
      - host: "api.microservice.com"
        match_with:
          path:
            patterns: ["/auth/*"]
        upstreams:
          - ip: "10.0.1.10"
            port: 3001
      - host: "api.microservice.com"
        match_with:
          path:
            patterns: ["/users/*"]
        upstreams:
          - ip: "10.0.1.20"
            port: 3002
      - host: "api.microservice.com"
        match_with:
          path:
            patterns: ["/payments/*"]
        upstreams:
          - ip: "10.0.1.30"
            port: 3003
    worker_threads: 6

  # Internal service mesh (HTTP)
  - host: "0.0.0.0:8080"
    ssl: null
    routes:
      - host: "internal.microservice.com"
        upstreams:
          - ip: "10.0.2.10"
            port: 4001
          - ip: "10.0.2.20"
            port: 4002
    worker_threads: 2

  # Health checks and monitoring (HTTP)
  - host: "0.0.0.0:9090"
    ssl: null
    routes:
      - host: "health.microservice.com"
        upstreams:
          - ip: "10.0.3.1"
            port: 8080
    worker_threads: 1
```

## Legacy Configuration (Still Supported)

The old format continues to work with deprecation warnings:

```yaml
# Legacy format - automatically migrated
service_name: "legacy-config"
server:
  ssl_enabled: false
  https_address: "0.0.0.0:8080"
  http_address: "0.0.0.0:8081"

routes:
  - host: "example.com"
    upstreams:
      - ip: "10.0.1.1"
        port: 3000
```

This is automatically converted to:
```yaml
# Equivalent new format
proxies:
  - host: "0.0.0.0:8080"
    ssl: null
    routes:
      - host: "example.com"
        upstreams:
          - ip: "10.0.1.1"
            port: 3000
  - host: "0.0.0.0:8081"
    ssl: null
    routes:
      - host: "example.com"
        upstreams:
          - ip: "10.0.1.1"
            port: 3000
```

## Command Line Usage

All command-line flags continue to work for backward compatibility, but only affect the migrated proxy configurations:

```bash
# Legacy flags (still work)
proksi --server.ssl_disabled --server.https_address 127.0.0.1:8080

# New approach: Use configuration files for multiple proxies
proksi --config-path ./configs/
```

## Migration Guide

1. **Keep existing configuration**: No changes needed immediately
2. **Gradual migration**: Add `proxies` array alongside existing `server` config
3. **Full migration**: Remove `server` config and use only `proxies`
4. **Validation**: Use `proksi --test` to validate new configuration

The new architecture provides much more flexibility while maintaining full backward compatibility.