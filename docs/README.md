# Getting started

For a quick introduction, check the ASCII recording of a small configuration file for Proksi: https://asciinema.org/a/ORhG5Na2SHIBI8TH2mPPUHMVZ

<!-- [![asciicast](https://asciinema.org/a/ORhG5Na2SHIBI8TH2mPPUHMVZ.svg)](https://asciinema.org/a/ORhG5Na2SHIBI8TH2mPPUHMVZ) -->

***



<div data-full-width="true">

<figure><picture><source srcset="/assets/simple-flow-white-2.png" media="(prefers-color-scheme: dark)"><img src="/assets/simple-flow-white-2-light.png" alt="" width="563"></picture><figcaption><p>Proksi can be your reverse proxy or a load balancer in front of your public IPs</p></figcaption></figure>

</div>

**Proksi** is a **simple**, **lightweight**, and easy-to-use proxy server that proxies HTTP, HTTPS and TCP, with certificates handled for you (Let's Encrypt, your own files, or Kubernetes secrets). It is designed to be used as a standalone proxy server or as a component in a larger system. Proksi is written in [Rust](https://www.rust-lang.org/) and uses [Cloudflare Pingora](https://blog.cloudflare.com/pingora-open-source) as its core networking library.



### Quick start

Create a configuration file or specify a direct file path:

```bash
# Option 1: Create a directory and configuration file
mkdir config
touch config/proksi.hcl

# Option 2: Create a configuration file anywhere with any name
touch my-proxy-config.yml
```

Add a simple route, let's try `docs.proksi.info` as our proxy route:

```hcl
#
lets_encrypt {
  enabled = true  # You must explicitly enable Let's Encrypt
  staging = true
  email = "my@email.com"
}


paths {
  # Where to save certificates?
  lets_encrypt = "./"

  # You can even use functions here
  # lets_encrypt = env("HOME")
}

# The hosts Proksi should handle
# You might need to edit your /etc/hosts file here.
host "mysite.localhost" {
  tls {
    # Let's Encrypt can't issue certificates for localhost: use a self-signed one instead
    self_signed_on_failure = true
  }

  location "/" {
    upstream "docs.proksi.info:443" {
      # An external HTTPS service expects its own name, not mysite.localhost
      sni = "docs.proksi.info"
      headers {
        add = { "Host" = "docs.proksi.info" }
      }
    }
  }
}
```

Then run Proksi with your configuration:

```bash
# Direct file path (any filename)
proksi -c ./config/proksi.hcl
proksi -c ./my-proxy-config.yml

# Directory path (looks for proksi.hcl, proksi.yml, proksi.yaml)
proksi -c ./config/

# No configuration (minimal defaults, Let's Encrypt disabled)
proksi
```



## Features

Of the many features **Proksi** offers is the ability to **load balance** to your infrastructure or **any IP** that supports your host configurations. Other features of **Proksi** also include:

1. Automatic **Docker** and **Docker Swarm** service discovery through labels, and a **Kubernetes** ingress controller
2. Built-in most common middlewares such as **OAuth**, **Rate Limiting**, **CDN** **Caching** and others
3. The ability of running it as a single binary in your system
4. Automatic **SSL** through **Let's Encrypt** and redirection from HTTP to HTTPS
5. Configuration through **HCL with support for functions**
6. **TCP** proxying, by port or by name (SNI), with TLS terminated or passed through
7. An **admin API** with Prometheus metrics and live stats

See [Features](general/features.md) for the full list.
