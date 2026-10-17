# ADR 0006: Upstream name resolution

- Status: accepted
- Date: 2026-10-17
- Related: [ADR 0003](adrs/0003-configuration-model.md) (hosts, locations and upstreams), [ADR 0005](adrs/0005-kubernetes-ingress.md) (upstreams that follow pods)

## Context

An upstream can be written as an address (`10.0.0.5:8080`) or a name (`app:8080`, `api.example.com:443`). Names are resolved once, when the host or `tcp` block is built, and proksi only knows the addresses from then on. When a name starts pointing elsewhere (a container recreated with a new address, a service moved to another provider), requests keep going to the old addresses and fail until the host changes or proksi restarts. A name that doesn't resolve when the host is built (a container that isn't up yet) makes the whole location invalid.

Proxies handle names in one of two ways:

- **Resolving on a schedule**, and replacing the addresses when the answer changes. Each address is an upstream of its own, balanced and health checked, and requests never wait for DNS.
- **Resolving on every new connection.** Never stale, but each new connection waits for a lookup, a DNS outage fails requests, and several addresses behind a name can't be balanced or health checked one by one.

## Decision

Upstream names are resolved again on a schedule, and when connecting to one of their addresses fails.

- **Resolver.** The system's, as for any other program in the same place: `/etc/hosts`, Docker's embedded DNS, the cluster DNS in Kubernetes.
- **Every 5 seconds**, every name is resolved again. Record TTLs aren't used to wait longer: Docker's DNS answers with a TTL of 10 minutes, which would keep a recreated container unreachable for as long.
- **On a failed connection** to an upstream with a name, its names are resolved again right away (at most once a second), so a recreated container is usually followed by the next request.
- **Every address of a name is an upstream**, with the weight and settings of the upstream it came from, balanced and health checked like the others. Addresses that stay keep their health.
- **Lookups that fail keep the last addresses.** A DNS outage (a timeout, a server failure) doesn't empty an upstream.
- **A name that doesn't exist has no addresses.** The resolver's answer that a name doesn't exist (`NXDOMAIN`, or no address records) is authoritative, as when a container is removed: its last addresses are dropped, so connections fail right away instead of waiting on an address nobody answers. A name without addresses (never resolved, or no longer existing) makes the location answer `503`, instead of being invalid. A request that finds no upstream resolves the names again first (at most every 100 milliseconds) and is sent to the new address if there's one, so a replaced container is followed as soon as it's up.
- Upstreams written as addresses are never resolved.
- The `Host` header and SNI sent to an upstream are unchanged: they come from the upstream's settings, not from the address chosen.

## Consequences

- Recreated containers and moved services are followed within seconds, without restarting proksi or changing its configuration.
- Hosts whose upstreams aren't up yet are served (with `503`) instead of skipped, and start working when their names resolve.
- A name is looked up every 5 seconds per location using it. Lookups are answered by local resolvers in most setups, and they happen in the background, never while handling a request.
- A typo in an upstream name no longer makes a location invalid: it's logged, and the location answers `503`.

### Not part of this decision

- `SRV` records (ports from DNS).
- Configuring the interval, or using record TTLs when they are shorter. 5 seconds covers the cases this is for, and either can be added later without changing the behaviour above.
