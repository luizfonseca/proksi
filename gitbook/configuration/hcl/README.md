---
description: Configuration based on the Hashicorp Configuration Language
---

# HCL

## Configuration

Proksi can be configured using HCL ([HashiCorp Configuration Language](https://github.com/hashicorp/hcl/blob/main/hclsyntax/spec.md)). This is the recommended way to configure Proksi, as it is more human-readable and easier to work with than JSON or YAML as well as it offers `functions` that you can use throughout your configuration:

```bash
touch proksi.hcl
```

```hcl
worker_threads = env("WORKER_THREADS")

lets_encrypt {
  enabled = true
  email = env("LETS_ENCRYPT_EMAIL")
  staging = true
}

paths {
  lets_encrypt = env("LETS_ENCRYPT_PATH")
}

// You can split your websites into separate files
routes = [
  import("./sites/mywebsite.com.hcl"),
  import("./sites/myotherwebsite.co.uk.hcl")
]

// Or you can define them here
routes = [
  {
    host = "cdn.example.com"
    ssl_certificate = {
      // Useful for development
      self_signed_on_failure = true
    }
    upstreams = [{
      ip = "example.com"
      port = 443

      headers = {
        add = [
          { name = "Host", value = "example.com" },
          { name = "X-Proxy-For", value = "cdn.example.com" }
        ]
      }
    }]
  }
]
```



