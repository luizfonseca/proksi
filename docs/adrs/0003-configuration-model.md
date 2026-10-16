# ADR 0003: Hosts, locations and TCP in the configuration

- Status: accepted
- Date: 2026-10-16
- Related: [ADR 0001](adrs/0001-route-sources.md) (route sources), [#381](https://github.com/luizfonseca/proksi/issues/381) (Kubernetes ingress), [#383](https://github.com/luizfonseca/proksi/issues/383) (WASM plugins)

## Context

Common reverse proxy setups need things proksi can't express today:

- **TCP routing.** Services that speak protocols other than HTTP (over TLS or not), possibly on the same hostname and port as HTTP services. proksi only has HTTP services.
- **More than one route per host.** A host has a single route; its path patterns only filter requests (no match is a 404), they don't choose between routes. Parts of a host often need different settings, such as a stricter rate limit on a login endpoint.
- **Rate limiting**, and **request body limits** (read before the upstream sees the request, so slow clients hold proksi rather than the application).

The configuration also doesn't use HCL the way HCL users expect. Routes are a list of nested objects, which reads like JSON:

```hcl
routes = [
  {
    host = "example.com"
    upstreams = [{ ip = "10.0.0.1", port = 3000 }]
    match_with = { path = { patterns = ["/api/*"] } }
  }
]
```

Terraform-style HCL uses labelled blocks, where the label identifies the thing being configured.

## Decision

### The model

```
host "<name>"          names, TLS (certificate, Let's Encrypt, TLS versions), host-wide HTTP middleware
├── location "<path>"  HTTP, chosen by path: middleware + upstreams
└── tcp "<name>"       TCP, chosen by port (+ SNI/ALPN): upstreams
tcp "<name>"           plain TCP without TLS or hostname, chosen by port only
```

A host owns what's decided during the TLS handshake, before the path is known: its names and its certificate. Its traffic is handled by `location` blocks (HTTP) and `tcp` blocks, which share the host's certificate.

```hcl
host "api.example.com" {
  aliases = ["www.api.example.com"]   # served with the same certificate

  tls {
    lets_encrypt {                     # or: certificate { pem = "...", key = "..." }, or self_signed = true
      staging = false
    }
    min_version = "1.2"
  }

  headers {                            # host-wide, applies to every location
    response {
      remove = ["Server"]
    }
  }

  location "/login" {
    rate_limit {
      requests = 30
      per      = "1m"
    }
    upstream "app:8080" {}
  }

  location "/*" {
    rate_limit {
      requests = 120
      per      = "1m"
    }
    body_limit {
      max    = "1MiB"
      buffer = true
    }
    upstream "app:8080" { weight = 2 }
    upstream "app-2:8080" {}

    plugin "custom" {
      source     = "../plugin.wasm"
      properties = { any = "json" }    # defined by the plugin, so free-form
    }
  }

  tcp "realtime" {
    listen = "0.0.0.0:443"
    alpn   = ["example-realtime"]
    upstream "realtime:7878" {}
  }
}

tcp "postgres" {
  listen = "0.0.0.0:5432"
  upstream "db:5432" {}
}
```

An `upstream` label is an address (`host:port`, `[ipv6]:port`). Its body holds the optional settings: `weight`, `network` (docker), `sni` and request `headers`.

### Locations

`location` is borrowed from nginx because it's familiar, but matching is proksi's own:

- Labels use the path patterns proksi already supports (`/api/*`, `/users/:id`). Each host builds one path tree from its locations, and a request goes to the most specific match: a static path beats a parameter, which beats a wildcard.
- A request matching no location gets a 404, as today.
- nginx modifiers (`= /path`, `~ regex`, `^~ /path`) are rejected with an error saying so, instead of being read as literal paths.
- Two locations with the same label in one host are rejected (the HCL parser keeps both, so this is detectable).

### Any host, and hosts without TLS

- `host "*"` serves requests whose `Host` header has no host of its own, for running proksi on an address without caring about names. A named host always wins over `*`. An IP is already a valid host name (`host "192.168.1.10"`), since hosts are matched on the `Host` header.
- `tls = false` serves a host over plain HTTP on `server.http_address`, instead of redirecting to HTTPS, and gets no certificate (no Let's Encrypt, no self-signed). Hosts with TLS keep redirecting from HTTP to HTTPS.
- `listen = ["ip:port", ...]` sets where a host is served, instead of the defaults (`server.https_address`, or `server.http_address` with `tls = false`). Which ports a host uses belongs to the host, not to a global setting: proksi opens the ports its hosts need, so with only `tls = false` hosts the HTTPS port isn't opened at all ([#293](https://github.com/luizfonseca/proksi/issues/293)). The protocol of a port follows from `tls`, so a host with TLS and a host without it can't share an address.
- Hosts from `routes`, docker labels and the store use the default addresses, since they can appear while proksi runs and ports are opened at startup. Port 80 stays open for hosts with TLS (redirects, and Let's Encrypt only validates on port 80).

```hcl
host "*" {
  tls = false
  location "/" {
    upstream "app:3000" {}
  }
}
```

### Building blocks

Everything inside a location besides upstreams is a building block: `headers`, `rate_limit`, `body_limit`, `cache`, `basic_auth`, `jwt`, `oauth2`, `request_id` and `plugin "<name>"`. Each block is one Rust type that:

1. Deserializes its HCL block into a struct.
2. Validates it when the configuration loads, so a bad block rejects the configuration (and a reload, per ADR 0001).
3. Implements the request stages it needs through the existing `MiddlewarePlugin` trait (request, upstream request, response), plus a request body stage for `body_limit`.

Blocks are built once per location when routes are applied, so their state (rate limit counters) belongs to the location. A block that answers the request (401, 413, 429…) ends it.

Blocks run by **priority**, from 0 (first) to 100 (last). Every block has a default, and any block can override it with `priority = <0-100>`:

| Block | Default priority |
| --- | --- |
| `request_id` | 0 |
| `rate_limit` | 0 |
| `body_limit` | 10 |
| `basic_auth`, `jwt`, `oauth2` | 20 |
| `headers` | 50 |
| `cache` | 60 |
| `plugin "<name>"` | 100 |

```hcl
location "/*" {
  rate_limit {
    requests = 120
    per      = "1m"
  }

  plugin "geo-block" {
    source   = "../geo.wasm"
    priority = 5                     # before body limits and authentication
  }
}
```

- `rate_limit` runs before authentication by default, so it also protects login endpoints from brute force. `request_id` also runs at 0, so rejected requests still get an id.
- Blocks with the same priority run in the order of the table, then `plugin` blocks in the order they're written. Host-wide blocks run before a location's own blocks with the same priority.
- Response stages run in reverse order, so the first block to see a request is the last to see its response.
- Priorities are needed because HCL doesn't keep the order between blocks of different types, so the order they're written in can't be used.

Middleware applies to HTTP only. HTTP blocks inside `tcp` are rejected.

Adding a building block is a new file with its tests and docs; routing doesn't change.

### TCP

A `tcp` block forwards connections to its upstreams, with the same load balancing and health checks as HTTP.

- On its own port, a `tcp` block inside a host terminates TLS with the host's certificate, and the upstream receives plain TCP. With `tls = "passthrough"`, TLS is forwarded untouched and the connection is chosen by SNI only.
- On a port shared with HTTPS (443), SNI can't tell HTTP and TCP apart for the same host, so the `tcp` block must set `alpn`: connections asking for one of its protocols go to it, the rest to the host's locations. A shared-port `tcp` block without `alpn` is rejected when the host also has locations. Passthrough can't share a port with locations, since proksi would need to decrypt the connection to hand it to HTTP.
- A top-level `tcp` block has no TLS and no hostname, and is chosen by port only.

Sharing 443 needs proksi to accept the connection itself, read SNI and ALPN from the TLS ClientHello, and then either finish the handshake and hand the connection to the HTTP proxy or forward it. Pingora doesn't do this out of the box, so `tcp` on its own port comes first.

### Compatibility

- `routes = [...]` keeps working and logs a deprecation warning. Each route becomes a host with one location: `/*`, or one location per `match_with` pattern, all with the same settings.
- YAML uses the same nesting as maps (`host: { "a.com": { location: { "/*": { ... } } } }`).
- Docker labels keep their current names and become a host with a `location "/*"`. Labels for locations and TCP can be added later.
- The store (ADR 0001) stores whole hosts as `schema_version: 2`. Version 1 routes are still read and converted like `routes = [...]`.
- The unit merged between sources (ADR 0001) is still the host: the highest priority source defines all of a host's locations and TCP blocks.

## Consequences

- Hosts can have several locations with their own middleware, and proksi can route TCP, which covers the setup that started this.
- The configuration reads like other HCL, and maps onto Kubernetes Ingress rules (host and path) for #381.
- Built-in features are added as building blocks without touching routing.
- Two configuration formats are supported until `routes = [...]` is removed in a future major version.

### Order of work

Each step ships on its own:

1. The model, `host` and `location` in HCL/YAML, with `routes = [...]`, docker labels and stored routes converted into it.
2. Choosing a location by path within a host.
3. `host "*"`, `tls = false` (plain HTTP) and `listen` addresses per host.
4. The existing plugins, headers and cache as building blocks.
5. `rate_limit`, then `body_limit`.
6. `tcp` on its own port.
7. `tcp` sharing 443 through SNI/ALPN.

Steps 1–2 cover more than one route per host, 3 running proksi without names or TLS (#293), 5 rate and body limits, and 6–7 TCP routing.

### Not part of this decision

- Sharing rate limit counters between instances (a `rate_limit` option using the store, later).
- UDP. Pingora doesn't support it; `udp` stays free as a block name.
- Matching by headers, methods or query strings in locations.
