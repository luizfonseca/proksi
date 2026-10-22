# Vendored crates

## pingora-proxy

Unmodified `pingora-proxy` 0.9.0 from crates.io, with three changes (marked with
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

### A request body read by a filter (`src/proxy_trait.rs`, `src/proxy_h1.rs`, `src/proxy_h2.rs`, `src/proxy_custom.rs`)

Blocks that check or change a request's body (`body_rewrite`) read it whole in
`request_filter`, before anything is sent upstream. Pingora only sends a body it read
before proxying from its retry buffer, which keeps 64 KiB: a larger one would never be
sent. `ProxyHttp::buffered_request_body` gives Pingora the body to send instead (and
again on a retry); proksi sets its `Content-Length` in `upstream_request_filter`.

Remove this directory and the `[patch.crates-io]` entry in the root `Cargo.toml`
once a pingora release includes the websocket fix, lets proxies share a connector,
and sends a body read by a filter.
