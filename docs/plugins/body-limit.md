---
description: Limits the size of request bodies
---

# Body Limit

Limits the size of the request body a location accepts. Requests with a larger body get a `413 Payload Too Large`.

By default, `body_limit` runs right after `rate_limit` (priority `10`), before authentication.

## Options

<table><thead><tr><th width="205">Name</th><th>Description</th></tr></thead><tbody><tr><td><code>max</code></td><td>The largest body accepted, such as <code>"512KiB"</code>, <code>"1MiB"</code> or <code>"10MB"</code> (units: <code>B</code>, <code>KB</code>, <code>MB</code>, <code>GB</code>, <code>KiB</code>, <code>MiB</code>, <code>GiB</code>)</td></tr><tr><td><code>priority</code></td><td>When it runs among the blocks of the location (default: <code>10</code>)</td></tr></tbody></table>

### Usage

{% code title="proksi.hcl" overflow="wrap" lineNumbers="true" %}
```hcl
host "mywebsite.com" {
  location "/upload" {
    upstream "localhost:3000" {}

    body_limit {
      max = "10MiB"
    }
  }

  location "/" {
    upstream "localhost:3000" {}

    body_limit {
      max = "1MiB"
    }
  }
}
```
{% endcode %}

### Good to know

- Requests that announce a larger body (`Content-Length`) are answered right away, without reaching the upstream.
- Bodies without a `Content-Length` (chunked) are counted as they are proxied, and the request ends once it goes over `max`: the upstream may have received part of it.
- The body is proxied as it arrives, not read completely before reaching the upstream.
