# ADR 0002: Coordinated certificate issuance

- Status: accepted
- Date: 2026-10-16
- Related: [ADR 0001](adrs/0001-route-sources.md) (route sources and store-backed routes), [#387](https://github.com/luizfonseca/proksi/pull/387) (shared ACME account for SQL stores)

## Context

Every proksi instance runs the Let's Encrypt service. With a shared store (redis, sqlite, postgres), instances already share certificates, ACME challenges and the ACME account (#387 for SQL stores), so any instance can answer an HTTP-01 challenge. What they don't share is the decision of _who_ orders a certificate:

- Every 20 seconds, each instance orders a certificate for every route that has none in the store. Instances starting together order the same domain at the same time.
- Each instance checks expiry on its own timer and renews certificates expiring within 30 days, so renewals race the same way.
- Let's Encrypt allows 5 duplicate certificates per week, so a few restarts of a few replicas can hit the limit, after which the domain gets no certificate at all.
- The redis store caches certificates in memory and never refreshes them. When one instance renews a certificate, the others keep serving the old one until they restart, possibly past its expiry. The SQL stores refresh certificates every 20 seconds, but never drop deleted ones.
- Two instances starting on an empty store can both create an ACME account; the last one written wins, which is harmless but wasteful.

## Decision

### Leases in the store, not consensus

Instances coordinate through a **lease**: a named lock held by one instance until it releases it or it expires. The store is already the source of truth that every instance must reach, so it can hold the lease.

Consensus (Raft) would let instances agree without a shared store, but they'd need to know about each other (membership, ports, persisted logs), and proksi already depends on the store being reachable. Leases are enough because ordering a certificate doesn't need _exactly-once_: two instances ordering the same domain both get a valid certificate, and the last write wins. The lease only has to make duplicates rare, which keeps proksi well under the rate limits.

Since a duplicate is harmless, there's no fencing: an instance whose lease expired mid-order still finishes and writes its certificate.

### The store trait

```rust
/// Takes the lease `name` for `holder`, if nobody else holds it (its holder can take it again,
/// extending it). Expires after `ttl`
async fn try_lock(&self, name: &str, holder: &str, ttl: Duration) -> Result<bool, StoreError>;
/// Releases the lease, only if `holder` still holds it
async fn unlock(&self, name: &str, holder: &str) -> Result<(), StoreError>;
```

The holder is the replica id reported in the heartbeats (ADR 0001). Expiry always uses the store's clock, never the instance's:

| Store | Lease |
| --- | --- |
| memory | In-process map with expiry (single instance; still prevents two tasks racing) |
| redis | `proksi:lock:<name>` set to `<holder>` with `PX <ttl>` by a Lua script (if missing, or extended if `holder` has it); `unlock` is a Lua script that deletes the key only if its value is `holder` |
| sqlite / postgres | `proksi_locks (name, holder, expires_at)` in milliseconds by the database's clock: `INSERT … ON CONFLICT (name) DO UPDATE SET holder = …, expires_at = … WHERE proksi_locks.holder = <holder> OR proksi_locks.expires_at <= <now>`, held if a row was written. `unlock` is `DELETE … WHERE name = $1 AND holder = $2` |

Postgres advisory locks are tied to a connection, which doesn't fit a connection pool, and sqlite has none, so both SQL stores use the table.

### Issuing and renewing

There's one lease per domain, `acme:<domain>`. Compared with electing a single leader, work spreads across instances, a stuck order doesn't block other domains, and there's no failover delay.

For each domain that needs a certificate (missing, or expiring within 30 days), an instance:

1. Takes `acme:<domain>` with a 10 minute TTL. If another instance holds it, it skips the domain until its next check.
2. Reads the certificate from the store again, since another instance may have just issued or renewed it, and stops if it's no longer needed.
3. Orders the certificate (or creates the self-signed fallback, as today), saves it in the store, and bumps the certificate revision.
4. Releases the lease, also when the order fails. If the instance dies, the lease expires and another instance picks the domain up.

Creating the ACME account at startup takes an `acme:account` lease the same way. An instance that finds it held waits for it (up to a minute), then reads the account the other instance created.

When the store can't be reached, no lease can be taken, and the domain is checked again next time.

### Syncing certificates

Like routes, certificates get a **revision**, bumped atomically with every certificate write (`proksi:certificates:revision` in redis, a `certificates_revision` row in `proksi_meta` for SQL). The store sync from ADR 0001 also polls it every `sync_interval_secs`, and when it changes, reloads all certificates into the instance's cache, replacing what's there in place (a handshake never finds the cache empty). A renewed certificate reaches every instance within one sync interval.

On a cache miss (a certificate issued by another instance since the last sync), the TLS handshake still reads the store, as today.

## Consequences

- Each domain is normally ordered or renewed once, by one instance, however many are running.
- Renewed certificates reach every instance without a restart.
- A store outage pauses issuance and renewal (there's no lease to take). Certificates already loaded keep being served, and renewals start 30 days before expiry, so there's plenty of margin.
- An order that takes longer than the lease TTL can be duplicated by another instance. This is rare and harmless.
- The memory store stays single-instance: two instances with memory stores can't coordinate, as today.

### Testing

- Store contract tests for leases, run against every backend: a second holder is refused, the lease can be taken after it expires, only the holder can release it.
- Certificate revisions in the store contract tests: every write bumps it, and a sync replaces the cache (a renewed certificate is picked up, a deleted one dropped).
- Issuance goes through a small trait around the ACME client, so a test can run two "instances" against the same store with a fake client and assert that each domain is ordered once, that a failed order releases the lease, and that an expired lease is picked up.

### Not part of this decision

- An end-to-end test against [Pebble](https://github.com/letsencrypt/pebble) (Let's Encrypt's test server).
- Showing lease holders and certificate status in the admin API (#49).
- Not blocking TLS handshakes on the store during an outage (an existing issue with redis; to be fixed separately).
- DNS-01 challenges and wildcard certificates.
