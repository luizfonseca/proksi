# Headers

Proksi can change the headers of responses (sent to the client) and of requests (sent to the upstream).

## Response headers

Response headers can be added or removed for a whole host or for a single location:

```hcl
host "example.com" {
  # Every response of the host
  headers {
    response {
      add    = { "X-Api-Version" = "2" }
      remove = ["Server"]
    }
  }

  location "/assets" {
    upstream "10.0.0.1:3000" {}

    # Only responses of this location, applied after the host's
    headers {
      response {
        add = { "Cache-Control" = "public, max-age=3600" }
      }
    }
  }
}
```

A location's headers are applied after the host's, so a location can override a header added by its host.

## Request headers

Headers added to the request sent to an upstream are set on the upstream itself. This is mostly useful for external services that expect a specific `Host` header:

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

The `Host` header of the original request is forwarded unchanged unless an upstream sets it.

## Forwarded headers

Upstreams receive who the client is and how it connected:

| Header | |
| --- | --- |
| `X-Forwarded-For` | The client's address |
| `X-Forwarded-Proto` | `https` or `http` |
| `X-Forwarded-Host` | The `Host` the client asked for |
| `X-Real-IP` | The client's address |

A client's own `X-Forwarded-*` headers are replaced, so it can't pass for another address. Behind proxies listed in `server.trusted_proxies`, their headers are kept and the proxy's address is appended to `X-Forwarded-For` (`X-Real-IP` is then the client they forwarded, as for [rate limits](plugins/rate-limit.md)).

They're set before a location's `headers`, which can still change or remove them.
