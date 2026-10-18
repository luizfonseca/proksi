# Vendored crates

## pingora-proxy

Unmodified `pingora-proxy` 0.9.0 from crates.io, with two changes (marked with
`proksi:`).

### Websockets closed after their upgrade (`src/proxy_h1.rs`)

Without it, a websocket can be closed right after a successful upgrade when the
upstream's `101` response is read before the upgrade request's (empty) body has
been processed, which happens under load:
https://github.com/cloudflare/pingora/issues/946 (fix proposed in
https://github.com/cloudflare/pingora/pull/1016). `test_many_concurrent_websockets`
in `crates/proksi/tests/e2e.rs` covers it.

### One upstream connector for every proxy (`src/lib.rs`)

Each `HttpProxy` creates its own upstream connector, and each connector loads its
own copy of the system's trusted certificates (about 1 MB). proksi creates a proxy
per listening service (HTTP, HTTPS, TLS passthrough addresses), so they share one.

Remove this directory and the `[patch.crates-io]` entry in the root `Cargo.toml`
once a pingora release includes the websocket fix and lets proxies share a
connector.
