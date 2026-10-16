# TCP

Proksi can also forward TCP connections, for services that don't speak HTTP (databases, game servers, message brokers...). A `tcp` block accepts connections on its own address and forwards them to its upstreams, with the same load balancing (`weight`) and health checks as HTTP.

## Plain TCP

A `tcp` block at the top level forwards connections as they are, chosen by the port they arrive on:

```hcl
tcp "game" {
  listen = "0.0.0.0:7777"

  upstream "10.0.0.1:7777" { weight = 2 }
  upstream "10.0.0.2:7777" {}
}
```

## TLS with the certificate of a host

A `tcp` block inside a host terminates TLS with the host's certificate (Let's Encrypt, certificate files or self-signed, as for HTTPS), and the upstream receives plain TCP. Clients connect with TLS and the name of the host:

```hcl
host "db.example.com" {
  tcp "postgres" {
    listen = "0.0.0.0:5433"
    upstream "10.0.0.5:5432" {}
  }
}
```

The `tcp` blocks of different hosts can share an address: the host is chosen by the name the client asks for (SNI). A client asking for any other name isn't forwarded.

## TLS passthrough

With `tls = "passthrough"`, TLS is forwarded untouched, and the upstream terminates it with its own certificate:

```hcl
host "secure.example.com" {
  tcp "app" {
    listen = "0.0.0.0:8443"
    tls    = "passthrough"
    upstream "10.0.0.6:443" {}
  }
}
```

In a host with `tls = false`, a `tcp` block is plain TCP.

## Good to know

- Each `tcp` block needs its own address: it can't be used by HTTP or HTTPS hosts, or by other `tcp` blocks (except blocks of different hosts with TLS, see above).
- `tcp` blocks come from the configuration file (not from docker labels or the store), and are set up when Proksi starts: changing them restarts Proksi.
- Upstreams of a `tcp` block only take `weight`.
- Sharing port `443` between HTTPS and TCP (chosen by ALPN) isn't supported yet.
