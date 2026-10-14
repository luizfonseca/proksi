# Vendored crates

## pingora-proxy

Unmodified `pingora-proxy` 0.9.0 from crates.io, with one fix applied in
`src/proxy_h1.rs` (marked with `proksi:`).

Without it, a websocket can be closed right after a successful upgrade when the
upstream's `101` response is read before the upgrade request's (empty) body has
been processed, which happens under load:
https://github.com/cloudflare/pingora/issues/946 (fix proposed in
https://github.com/cloudflare/pingora/pull/1016).

Remove this directory and the `[patch.crates-io]` entry in the root `Cargo.toml`
once a pingora release includes the fix. `test_many_concurrent_websockets` in
`crates/proksi/tests/e2e.rs` covers it.
