---
description: Helpful functions you can use to optimize your configuration files.
---

# Functions

HCL supports functions that can be used to generate values in the configuration file. The following functions are supported:

### num_cpus

Returns the number of CPUs (logical cores) available on the system.

{% code title="proksi.hcl" lineNumbers="true" %}
```hcl
# You can use the num_cpus function to set the number of worker threads
# You can also use math expressions like num_cpus() - 2
worker_threads = num_cpus() - 2

logging {
  level = "info"
}
# ... other configuration
```
{% endcode %}



### env

You can use the env function to retrieve environment variables during the configuration parsing process of Proksi.&#x20;

This is particularly useful if you have secrets or other type of data that you do not want to be part of your `git` or repository:

#### Usage

`env(NAME: string)` where NAME is an environment variable present during Proksi's run.

{% code overflow="wrap" %}
```hcl
worker_threads = env("WORKERS_COUNT")

host "my-site.com" {
  location "/" {
    upstream "localhost:8009" {}

    basic_auth {
      user = env("BASIC_AUTH_USER")
      pass = env("BASIC_AUTH_PASS")
    }
  }
}
```
{% endcode %}

#### Errors

This function will throw an error if the environment variable is not set.



### import

Configuring Proksi in one single file is a no-brainer, but it works well only when you don't have hundreds of upstreams, routes, headers and reusable pieces that you want to commit or have others work on them too.

The `import` function is a way to help improve your configuration organization in the long term.



#### Usage

`import(RELATIVE_PATH: string)` where relative\_path <mark style="color:orange;">is always a path relative to the main configuration file</mark> (**not** relative to other imports).&#x20;

The reason the piece above is highlighted is because you can use `import` within `import`s and at the end what Proksi does is include them all together, so it needs to find them relative to itself.

{% code title="proksi.hcl" lineNumbers="true" %}
```hcl
worker_threads = 2

paths {
  lets_encrypt = "./"
}

host = {
  "my-site.com" = import("./sites/my-site.com.hcl")
}
```
{% endcode %}

And then, `./sites/my-site.com.hcl` holds the host's settings (what would go inside a `host "my-site.com" { ... }` block):

{% code title="sites/my-site.com.hcl" lineNumbers="true" %}
```hcl
# headers of every response
headers {
  response {
    add = { "server" = "cool-server" }
  }
}

location "/" {
  upstream "localhost:2000" {}
}
```
{% endcode %}

Hosts can be defined either with `host = { ... }` (to import them) or with `host "<name>" { ... }` blocks, not both in the same file.
