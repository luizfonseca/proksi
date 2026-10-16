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
