# HCL

## Configuration

Proksi can be configured using HCL ([HashiCorp Configuration Language](https://github.com/hashicorp/hcl/blob/main/hclsyntax/spec.md)). This is the recommended way to configure Proksi, as it is more human-readable and easier to work with than JSON or YAML as well as it offers `functions` that you can use throughout your configuration:

```bash
touch proksi.hcl
```

```hcl
worker_threads = env("WORKER_THREADS")

server {
  https_address = "0.0.0.0:5143"
}

lets_encrypt {
  enabled = true
  email = env("LETS_ENCRYPT_EMAIL")
  staging = true
}

paths {
  lets_encrypt = env("LETS_ENCRYPT_PATH")
}

host "cdn.example.com" {
  tls {
    // Useful for development
    self_signed_on_failure = true
  }

  location "/" {
    upstream "example.com:443" {
      sni = "example.com"

      headers {
        add = {
          "Host"        = "example.com"
          "X-Proxy-For" = "cdn.example.com"
        }
      }
    }
  }
}
```

## Hosts and locations

Each `host` block defines a host proksi accepts requests for. Its label is matched against the `Host` header of requests (case-insensitive) and is the name certificates are issued for. Inside it, `location` blocks choose how a request is handled based on its path:

```hcl
host "api.example.com" {
  tls {
    certificate {                 // certificate files, instead of Let's Encrypt
      pem = "/etc/proksi/certs/api.pem" // the certificate, then its intermediates
      key = "/etc/proksi/certs/api.key"
    }
    self_signed_on_failure = true // fall back to a self-signed certificate
  }

  headers {                       // every location of the host
    response {
      add    = { "X-Api-Version" = "2" }
      remove = ["Server"]
    }
  }

  location "/login" {
    upstream "auth:8080" {}
  }

  location "/" {
    upstream "app:8080" {}
    upstream "app-2:8080" {}

    request_id {}
    basic_auth {
      user = env("BASIC_AUTH_USER")
      pass = env("BASIC_AUTH_PASS")
    }
  }
}
```

**Hosts without TLS, and any host**

- `tls = false` serves a host over plain HTTP on `server.http_address` instead of redirecting to HTTPS, and no certificate is issued for it.
- `host "*"` serves requests whose `Host` has no host of its own. It needs `tls = false` (certificates are issued per name). Other wildcards (`*.example.com`) aren't supported yet.
- `listen = ["ip:port", ...]` sets the addresses a host is served on, instead of `server.https_address` (or `server.http_address` with `tls = false`). Proksi only opens the ports its hosts need: with only `tls = false` hosts, the HTTPS port isn't opened at all.
- A port is either HTTP or HTTPS: a host with TLS and a host without it can't share an address. Hosts with TLS also need `server.http_address` (port 80) for redirects and Let's Encrypt challenges.
- Hosts from `routes = [...]`, docker labels and the store use the default addresses, as they can appear while proksi runs (ports are opened at startup). A configuration change that needs other ports restarts proksi.

```hcl
host "*" {
  tls = false

  location "/" {
    upstream "app:8080" {}
  }
}
```

**Matching paths**

- A request goes to the **most specific** location of its host: a static path beats a parameter (`/users/:id`), which beats a wildcard (`/files/*`).
- A location matches its path **and everything below it**: `location "/login"` serves `/login`, `/login/` and `/login/callback`, but not `/logins`. `location "/"` serves whatever no other location matches.
- Paths are compared after collapsing repeated slashes and resolving `.` and `..`, so `//login` is served by `location "/login"`. The request is forwarded unchanged.
- A request matching no location gets a `404`.
- nginx modifiers (`=`, `~`, `~*`, `^~`) and regular expressions aren't supported.

**Inside a location**

| Block | Description |
| --- | --- |
| `upstream "<host:port>" { }` | Where requests are sent, load balanced (round-robin) with health checks. IPv6 addresses go in brackets (`[::1]:3000`). Optional settings: `weight` (1 by default), `network` (docker), `sni`, and `headers { add = { ... } }` added to the request sent to the upstream. |
| `headers { response { add = { ... }, remove = [...] } }` | Headers changed in responses, after the host's. |
| `cache { ... }` | Response caching, as in [caching](use-cases/cache.md). |
| `rewrite { strip_prefix, add_prefix }` or `rewrite { match, replace }` | The path sent to upstreams, by prefix or regular expression, see [rewriting paths](routing/upstreams.md#rewriting-paths). |
| `request_id { }`, `rate_limit { ... }`, `body_limit { ... }`, `basic_auth { ... }`, `oauth2 { ... }` | Plugins ([request id](plugins/request-id.md), [rate limit](plugins/rate-limit.md), [body limit](plugins/body-limit.md), [basic auth](plugins/basic-auth.md), [OAuth2](plugins/oauth2.md)), with the same settings as their `config` in `plugins = [...]`. |

**Order of blocks**

Everything in a location besides upstreams is a building block: plugins, `headers` and `cache`. Blocks run by **priority**, from `0` (first) to `100` (last), and a block that answers the request (e.g. a `401`) ends it. Every block has a default, and any block can change it with `priority = <0-100>`:

| Block | Default priority |
| --- | --- |
| `request_id`, `rate_limit` | 0 |
| `body_limit` | 10 |
| `basic_auth`, `oauth2` | 20 |
| `headers` | 50 |
| `cache` | 60 |

```hcl
location "/" {
  upstream "app:8080" {}

  basic_auth {
    user = env("BASIC_AUTH_USER")
    pass = env("BASIC_AUTH_PASS")
  }

  oauth2 {
    provider      = "github"
    client_id     = env("OAUTH_CLIENT_ID")
    client_secret = env("OAUTH_CLIENT_SECRET")
    jwt_secret    = env("OAUTH_JWT_SECRET")
    priority      = 10 # before basic_auth
  }
}
```

- Blocks with the same priority run in the order of the table.
- Responses go through the blocks in reverse order: the first block to see a request is the last to see its response.
- Invalid settings (e.g. `basic_auth` without `pass`) are rejected when the configuration is loaded.

Unknown attributes are rejected, so a typo fails when the configuration is loaded instead of being ignored. So are two locations matching the same paths (e.g. `/api` and `/api/*`).

**TCP**

`tcp "<name>" { listen = "ip:port", upstream "<host:port>" {} }` forwards TCP connections, at the top level (plain TCP) or inside a host (TLS with the host's certificate, or `tls = "passthrough"`). With `alpn = ["..."]`, a host can be HTTPS and TCP on the same port, chosen by the protocol the client asks for. See [TCP](routing/tcp.md).

To keep hosts in separate files, see [`import`](configuration/hcl/functions.md#import).

### Timeouts

`server.timeouts` sets how long proksi waits for clients (60 seconds each by default):

```hcl
server {
  timeouts {
    idle_secs  = 60 # for a request's headers, and between requests of a connection
    read_secs  = 60 # for each read of a request body, and websocket messages
    write_secs = 60 # for a client to read each write of a response
  }
}
```

A host or a location can change `read_secs` and `write_secs` for its requests (a location's override its host's). `idle_secs` only applies to the server: it's used before a request, so before its host is known.

```hcl
host "app.example.com" {
  timeouts {
    read_secs = 10
  }

  location "/upload" {
    timeouts {
      read_secs = 300  # large uploads from slow connections
    }
    upstream "app:8080" {}
  }

  location "/ws" {
    timeouts {
      read_secs = 120  # longer than the websocket clients' ping interval
    }
    upstream "app:8080" {}
  }
}
```

**Toward upstreams**, `timeouts { upstream { ... } }` sets how long proksi waits for them. In `server.timeouts` it applies to every location; a host or location changes it for its own, one value at a time (a location's override its host's, which override the server's):

```hcl
server {
  timeouts {
    upstream {
      connect_secs = 5   # to connect
      tls_secs     = 10  # for the TLS handshake of an HTTPS upstream, after connecting
      read_secs    = 360 # for each read of a response, including its headers
      write_secs   = 60  # for the upstream to read each write of a request
      idle_secs    = 360 # to keep an unused connection open, to reuse it
    }
  }
}

host "api.example.com" {
  timeouts {
    upstream {
      connect_secs = 1 # fail fast while a container is replaced
    }
  }

  location "/reports" {
    timeouts {
      upstream {
        read_secs = 900 # slow reports
      }
    }
    upstream "reports:8080" {}
  }
}
```

The values above are the defaults. An upstream that doesn't connect or answer in time gets the client a `504`; other upstream errors (e.g. a refused connection), a `502`.

`tcp` blocks have their own: `connect_timeout_secs` and `idle_timeout_secs` (see [TCP](routing/tcp.md)).

### TLS versions and ciphers

Clients connect with TLS 1.2 or 1.3. TLS 1.2 is limited to forward-secret cipher suites (ECDHE) with AES-GCM or ChaCha20, as in Mozilla's "intermediate" configuration; older ones, such as DHE, CBC or RSA key exchange, are refused. `server.tls` changes them for HTTPS and for `tcp` blocks that terminate TLS:

```hcl
server {
  tls {
    min_version = "1.3" # "1.2" by default

    # TLS 1.2 suites, by their OpenSSL names, in order of preference
    ciphers = [
      "ECDHE-ECDSA-AES256-GCM-SHA384",
      "ECDHE-RSA-AES256-GCM-SHA384",
    ]
  }
}
```

TLS 1.3 suites are always the standard ones, all of them strong. A cipher OpenSSL doesn't know makes the configuration invalid.

## Migrating from `routes = [...]`

`routes = [...]` still works, but is deprecated and logs a warning. Each route corresponds to a host:

```hcl
// Before
routes = [
  {
    host = "example.com"
    ssl_certificate = { self_signed_on_failure = true }
    upstreams = [{ ip = "10.0.0.1", port = 3000 }]
    match_with = { path = { patterns = ["/api/*"] } }
  }
]

// After
host "example.com" {
  tls {
    self_signed_on_failure = true
  }

  location "/api" {
    upstream "10.0.0.1:3000" {}
  }
}
```

Route patterns matched paths exactly (`/api` didn't match `/api/users`), while a location matches its path and everything below it. A host can't be defined both in `routes` and with a `host` block.
