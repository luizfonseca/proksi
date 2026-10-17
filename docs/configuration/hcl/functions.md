# Functions

HCL supports functions that can be used to generate values in the configuration file. The following functions are supported:

### num_cpus

Returns the number of CPUs (logical cores) available on the system.

**proksi.hcl**
```hcl
# You can use the num_cpus function to set the number of worker threads
# You can also use math expressions like num_cpus() - 2
worker_threads = num_cpus() - 2

logging {
  level = "info"
}
# ... other configuration
```



### env

You can use the env function to retrieve environment variables during the configuration parsing process of Proksi.

This is particularly useful if you have secrets or other type of data that you do not want to be part of your `git` or repository:

#### Usage

`env(NAME: string)` where NAME is an environment variable present during Proksi's run.

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

#### Errors

This function will throw an error if the environment variable is not set.



### import

Configuring Proksi in one single file is a no-brainer, but it works well only when you don't have hundreds of upstreams, routes, headers and reusable pieces that you want to commit or have others work on them too.

The `import` function is a way to help improve your configuration organization in the long term.



#### Usage

`import(RELATIVE_PATH: string)` where relative\_path is always a path relative to the main configuration file (**not** relative to other imports).

This matters because `import`s can be nested: Proksi includes them all together, so it looks for each of them relative to the main configuration file.

**proksi.hcl**
```hcl
worker_threads = 2

paths {
  lets_encrypt = "./"
}

host = {
  "my-site.com" = import("./sites/my-site.com.hcl")
}
```

And then, `./sites/my-site.com.hcl` holds the host's settings (what would go inside a `host "my-site.com" { ... }` block):

**sites/my-site.com.hcl**
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

Hosts can be defined either with `host = { ... }` (to import them) or with `host "<name>" { ... }` blocks, not both in the same file.
