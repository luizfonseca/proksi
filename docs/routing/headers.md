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
