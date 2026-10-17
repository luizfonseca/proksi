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
- When a lookup fails (the resolver doesn't answer), the last addresses are kept. A name that doesn't exist (a container that isn't up yet, or was removed) has no addresses: requests get a `503` right away. A request that finds no upstream resolves the name again first (at most every 100 milliseconds), so a new container gets requests as soon as it's up.

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

### Regular expressions

To change paths in any other way, `match` is a regular expression the path is matched against, and `replace` what a matching path becomes:

```hcl
location "/users" {
  # /users/42/profile?tab=posts is sent as /profiles/42?tab=posts
  rewrite {
    match   = "^/users/(\\d+)/profile$"
    replace = "/profiles/$1"
  }
  upstream "app:8080" {}
}
```

- `$1`, `$2`... are the groups of `match`. Named groups (`(?P<id>\\d+)`) are `$${id}` in HCL: `${...}` alone is HCL's own interpolation.
- A path that doesn't match is sent unchanged. The first match is replaced, so anchor the expression (`^...$`) to replace the whole path.
- `match` and `replace` can't be combined with `strip_prefix` or `add_prefix`.
- The syntax is Rust's [`regex`](https://docs.rs/regex/latest/regex/#syntax).

### Performance

A rewrite is only set up for the locations that have one: other requests don't run any of it. Patterns are compiled once, when the location is loaded (from the configuration, docker labels, Kubernetes or the store), never per request.

Measured per request: a prefix rewrite takes around 50 ns, a regular expression around 50 to 150 ns depending on its groups. A proxied request takes tens of microseconds, so either is negligible.

### Limitations

- Only the path is rewritten, not the query string or the host.
- Rust's regular expressions run in linear time, so a pattern can't make a request hang: in exchange, look-around (`(?=...)`, `(?!...)`) and backreferences (`\\1` in `match`) aren't supported.
- A compiled pattern is limited to 1 MB, as patterns can come from a shared store. Larger ones are refused.
- An invalid rewrite is refused when the configuration is loaded, or written through the admin API. One written to the store directly makes its host invalid: it's skipped and logged (and listed by the admin API), and the host's definition from another source, if any (the configuration file, docker labels), is served instead.
- Locations are still chosen by the client's path, before it's rewritten, and there are no redirects (`3xx`) based on a rewrite.

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
