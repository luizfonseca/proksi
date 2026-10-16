---
description: Limits how many requests each client can make
---

# Rate Limit

Limits the number of requests each client (by IP address) can make to a location in a period of time. Clients over the limit get a `429 Too Many Requests`, with a `Retry-After` header.

By default, `rate_limit` runs before any other block (priority `0`), so it also protects endpoints that use authentication from brute force.

## Options

<table><thead><tr><th width="205">Name</th><th>Description</th></tr></thead><tbody><tr><td><code>requests</code></td><td>Number of requests allowed for each client in each period</td></tr><tr><td><code>per</code></td><td>The period, such as <code>"10s"</code>, <code>"1m"</code> or <code>"1h"</code></td></tr><tr><td><code>priority</code></td><td>When it runs among the blocks of the location (default: <code>0</code>)</td></tr></tbody></table>

### Usage

{% code title="proksi.hcl" overflow="wrap" lineNumbers="true" %}
```hcl
host "mywebsite.com" {
  # A stricter limit for the login page
  location "/login" {
    upstream "localhost:3000" {}

    rate_limit {
      requests = 30
      per      = "1m"
    }
  }

  location "/" {
    upstream "localhost:3000" {}

    rate_limit {
      requests = 120
      per      = "1m"
    }
  }
}
```
{% endcode %}

### Good to know

- Each location counts its own requests, and requests are counted in fixed periods (a client can make `requests` in each period of `per`).
- Counts are kept in memory, on each Proksi server, and start over when the configuration changes.
- Counts use a fixed amount of memory and are estimates: with a very large number of clients in the same period, a client can be limited slightly before reaching `requests`, never after.
- Clients are identified by the IP address of the connection. Behind another proxy or load balancer, every request comes from its address, so the limit applies to all clients together.
