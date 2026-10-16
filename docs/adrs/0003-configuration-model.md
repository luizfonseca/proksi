# ADR 0003: Hosts, locations and TCP in the configuration

- Status: proposed
- Date: 2026-10-16
- Related: [ADR 0001](adrs/0001-route-sources.md) (route sources), [#381](https://github.com/luizfonseca/proksi/issues/381) (Kubernetes ingress), [#383](https://github.com/luizfonseca/proksi/issues/383) (WASM plugins)

## Context

A user comparing proksi with Traefik listed what their setup needs. Checked against the code, proksi is missing:

- **TCP routing by SNI.** Their game server speaks a binary protocol over TLS on the same hostname as their HTTP services. proksi only has HTTP services.
- **More than one route per host.** A host has a single route; its path patterns only filter requests (no match is a 404), they don't choose between routes. Their API needs a stricter rate limit on `/v1/session` than on the rest of the host.
- **Rate limiting**, and **request body limits** (a 1 MiB cap, read before the upstream sees the request, so slow clients hold proksi rather than the application).

Response headers and Let's Encrypt are already supported.

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
host "api.velorido.com" {
  aliases = ["www.api.velorido.com"]   # served with the same certificate

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

  location "/v1/session" {
    rate_limit {
      requests = 30
      per      = "1m"
    }
    upstream "directory:8080" {}
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
    upstream "directory:8080" { weight = 2 }
    upstream "directory-2:8080" {}

    plugin "custom" {
      source     = "../plugin.wasm"
      properties = { any = "json" }    # defined by the plugin, so free-form
    }
  }

  tcp "game" {
    listen = "0.0.0.0:443"
    alpn   = ["velorido-game"]
    upstream "game:7878" {}
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

### Building blocks

Everything inside a location besides upstreams is a building block: `headers`, `rate_limit`, `body_limit`, `cache`, `basic_auth`, `jwt`, `oauth2`, `request_id` and `plugin "<name>"`. Each block is one Rust type that:

1. Deserializes its HCL block into a struct.
2. Validates it when the configuration loads, so a bad block rejects the configuration (and a reload, per ADR 0001).
3. Implements the request stages it needs through the existing `MiddlewarePlugin` trait (request, upstream request, response), plus a request body stage for `body_limit`.

Blocks are built once per location when routes are applied, so their state (rate limit counters) belongs to the location. A block that answers the request (401, 413, 429…) ends it.

Blocks run in a **fixed order**: `request_id`, authentication (`basic_auth`, `jwt`, `oauth2`), `rate_limit`, `body_limit`, `headers`, `cache`, then `plugin` blocks in the order they're written. Host-wide blocks run before a location's own. The order is fixed because HCL doesn't keep the order between blocks of different types, and a fixed pipeline is easier to predict than one that depends on how the file is written.

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
- YAML uses the same nesting as maps (`hosts: { "a.com": { location: { "/*": { ... } } } }`).
- Docker labels keep their current names and become a host with a `location "/*"`. Labels for locations and TCP can be added later.
- The store (ADR 0001) stores whole hosts as `schema_version: 2`. Version 1 routes are still read and converted like `routes = [...]`.
- The unit merged between sources (ADR 0001) is still the host: the highest priority source defines all of a host's locations and TCP blocks.

## Consequences

- Hosts can have several locations with their own middleware, and proksi can route TCP, which covers the setup that started this.
- The configuration reads like other HCL, and maps onto Kubernetes Ingress rules (host and path) for #381.
- Built-in features are added as building blocks without touching routing.
- Two configuration formats are supported until `routes = [...]` is removed in a future major version.
- The fixed middleware order can't be changed per location. If that's ever needed, it can be added as an explicit setting.

### Order of work

Each step ships on its own:

1. The model, `host` and `location` in HCL/YAML, with `routes = [...]`, docker labels and stored routes converted into it.
2. Choosing a location by path within a host.
3. The existing plugins, headers and cache as building blocks.
4. `rate_limit`, then `body_limit`.
5. `tcp` on its own port.
6. `tcp` sharing 443 through SNI/ALPN.

Steps 1–2 cover more than one route per host, 4 rate and body limits, and 5–6 the game server.

### Not part of this decision

- Sharing rate limit counters between instances (a `rate_limit` option using the store, later).
- UDP. Pingora doesn't support it; `udp` stays free as a block name.
- Matching by headers, methods or query strings in locations.
- Per-location middleware order.
