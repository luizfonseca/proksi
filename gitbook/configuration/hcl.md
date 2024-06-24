---
description: Configuration based on the Hashicorp Configuration Language
---

# HCL

## Configuration 

Proksi can be configured using HCL (HashiCorp Configuration Language). This is the recommended way to configure Proksi, as it is more human-readable and easier to work with than JSON or YAML as well as it offers `functions` that you can use throughout your configuration:

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

#### HCL functions

HCL supports functions that can be used to generate values in the configuration file. The following functions are supported:

- `env(name: string)`: Get the value of an environment variable. If the environment variable is not set, an error is thrown.
- `import(path: string)`: Import another HCL file. The path is relative to the current file.
