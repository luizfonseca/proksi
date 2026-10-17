# Upstreams

Upstreams are where requests are sent. They are defined inside a `location`, labelled by their address (`host:port`, or `[ipv6]:port`):

```hcl
host "example.com" {
  location "/" {
    upstream "10.0.0.1:3000" {}
    upstream "10.0.0.2:3000" {}
    upstream "[2001:db8::1]:3000" {}
  }
}
```

A location can have as many upstreams as you need. Requests are load balanced between them (round-robin), in proportion to their `weight`:

```hcl
location "/" {
  upstream "10.0.0.1:3000" { weight = 3 } # 3 of every 4 requests
  upstream "10.0.0.2:3000" {}             # weight = 1 by default
}
```

## Names

An upstream can be a name instead of an address (`app:3000`, `api.example.com:443`), resolved by the system's resolver (`/etc/hosts`, Docker's DNS, the cluster's DNS):

- Every address of a name is an upstream of its own, with the same settings, load balanced and health checked like the others.
- Names are resolved again every 5 seconds, and right away when connecting to one of their addresses fails, so a recreated container or a moved service is followed without restarting Proksi.
- When a lookup fails (the resolver doesn't answer), the last addresses are kept. A name that doesn't exist (a container that isn't up yet, or was removed) has no addresses: requests get a `503` right away, and the name is resolved again, so a new container is followed within a second of being up.

## Rewriting paths

Requests are forwarded with their path unchanged. A location can change the path its upstreams receive, for an app served under a prefix it doesn't know about, or one that expects another prefix:

```hcl
host "example.com" {
  # /api/users?page=2 is sent as /users?page=2
  location "/api" {
    rewrite {
      strip_prefix = "/api"
    }
    upstream "api:8080" {}
  }

  # /legacy/orders is sent as /v2/orders
  location "/legacy" {
    rewrite {
      strip_prefix = "/legacy"
      add_prefix   = "/v2"
    }
    upstream "app:8080" {}
  }
}
```

- `strip_prefix` is removed from the start of the path, only at a segment boundary: `/api` is removed from `/api` and `/api/users`, not from `/apis`. Stripping the whole path leaves `/`.
- `add_prefix` is added to the start of the path, after `strip_prefix`.
- The query string is kept. Access logs show the path the client asked for.
- Prefixes are fixed paths; regular expressions aren't supported.

## Health checks

Every upstream is checked in the background (a TCP connection every 15 seconds). Upstreams that fail the check stop receiving requests until they are healthy again. If none of the upstreams of a location are healthy, the request gets a `503`.

## External services (TLS)

Upstreams on port `443` are reached over TLS. Set `sni` to the name of the service, and the `Host` header it expects, as Proksi forwards the `Host` of the original request:

```hcl
location "/" {
  upstream "docs.proksi.info:443" {
    sni = "docs.proksi.info"

    headers {
      add = { "Host" = "docs.proksi.info" }
    }
  }
}
```

## Settings

| Setting | Description |
| --- | --- |
| `weight` | The share of requests the upstream receives, relative to the others (`1` by default, up to `127`). |
| `sni` | The server name sent to an upstream using TLS (port `443`). |
| `headers { add = { ... } }` | Headers added to the request sent to the upstream. |
| `network` | The docker network to reach the upstream through (docker integration). |

## YAML

In YAML, upstreams are keyed by their address (not a list):

```yaml
host:
  example.com:
    location:
      /:
        upstream:
          "10.0.0.1:3000": {}
          "10.0.0.2:3000": {}
```
