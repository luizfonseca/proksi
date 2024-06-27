
![GitHub Release](https://img.shields.io/github/v/release/luizfonseca/proksi?style=for-the-badge)
![Crates.io MSRV](https://img.shields.io/crates/msrv/proksi?style=for-the-badge)
![Crates.io License](https://img.shields.io/crates/l/proksi?style=for-the-badge)
[![Crates.io Total Downloads](https://img.shields.io/crates/d/proksi?style=for-the-badge)](https://crates.io/crates/proksi)

# Proksi: Automatic SSL, HTTP, and DNS Proxy

<img src="./assets/discord.png" alt="discord-logo" width="200"/>


# About

Proksi is a simple, lightweight, and easy-to-use proxy server that automatically handles SSL, HTTP, and DNS traffic. It is designed to be used as a standalone proxy server or as a component in a larger system. Proksi is written in [Rust](https://www.rust-lang.org/) and uses [Pingora](https://github.com/cloudflare/pingora) as its core networking library.


# Quick start

1. Download the latest release from [https://github.com/luizfonseca/proksi/releases](https://github.com/luizfonseca/proksi/releases)
2. Create a configuration file named `proksi.hcl`
3. Add the following content to the file:

```hcl
#
lets_encrypt {
  enabled = true
  email = "my@email.com"
}

paths {
  # Where to save certificates?
  lets_encrypt = "./"
}

# A list of routes Proksi should handle
routes = [
  {
    # You might need to edit your /etc/hosts file here.
    host = "mysite.localhost",

    # Will create a certificate for mysite.localhost
    ssl_certificate =  {
      self_signed_on_failure = true
    }

    # Where to point mysite.localhost to
    upstreams = [{
      ip = "docs.proksi.info"
      port = 443

      headers = {
        add = [{ name = "Host", value = "docs.proksi.info" }]
    }]
  }
]
```
4. Run `proksi -c /path-where-proksi.hcl-is-located`

For more information or guides, please refer to the [documentation](https://docs.proksi.info).


# Documentation
Documentation for Proksi can be found at [https://docs.proksi.info](https://docs.proksi.info) which is also available in the [gitbook](./gitbook/) folder of this repository.


# Contributing
We welcome contributions to Proksi. If you have any **suggestions** or **ideas**, please feel free to open an issue or a pull request on the GitHub repository.

# License
Proksi is licensed under the [MIT License](https://github.com/luizfonseca/proksi/blob/main/LICENSE), the [Apache License 2.0](https://github.com/luizfonseca/proksi/blob/main/LICENSE-APACHE) and is free to use and modify.
