# Features

Proksi is evolving day by day, and new features are being added all the time. Below is a list of the features that are currently available in Proksi:

- [x] **HTTPS**: SSL termination, automatic redirect from HTTP and certificate renewal (Let's Encrypt)
- [x] **Plain HTTP** hosts (`tls = false`), for running behind another load balancer or on an IP address
- [x] **Hosts and locations**: the most specific location of a host serves each request (static paths, parameters and wildcards)
- [x] **Path rewriting** per location: strip or add a prefix to the path sent to upstreams
- [x] **Catch-all host** (`host "*"`) for names without a host of their own
- [x] **Listen addresses per host**: Proksi only opens the ports its hosts need
- [x] **Docker**: Swarm/Compose Label Support
- [x] **Load Balancing** (✅ Round Robin, ✅ Weighted Round Robin, ⛔︎ Least Connections)
- [X] Health Checks
- [x] **Upstream names** (`app:8080`) resolved again as their addresses change, following recreated containers
- [x] **Shared store** (Redis, SQLite, Postgres) to run many Proksi servers with the same routes and certificates
- [x] **Auto reload** of the configuration file: hosts applied live, other settings through an [upgrade](configuration/upgrades.md) that keeps connections open (Linux)
- [x] **HCL functions** Extensible through configuration with `env` and `import`
- [x] Header manipulation for DOWNSTREAM & UPSTREAM (add/replace, remove)
- [x] **Caching** in memory or on disk
- [X] **Basic** Authentication
- [X] **OAuth2** Authentication (GitHub and WorkOS)
- [X] **RequestId** Middleware
- [x] **Rate limiting** per location and client
- [x] **Body limits** per location
- [x] **TCP** proxying: plain, TLS with the host's certificate (chosen by SNI) and TLS passthrough
- [x] Sharing port `443` between HTTPS and TCP, chosen by name (SNI) or protocol (ALPN), with TLS terminated or passed through
- [x] **Kubernetes** ingress controller: `Ingress` resources served with their ready pods as upstreams, TLS secrets, plugins through annotations, status and events, with a Helm chart
- [x] **Metrics** in the Prometheus format, per host, location and upstream, on an admin listener with a token
- [x] **Admin API**: live stats (requests per second, latency percentiles), hosts, certificates, and routes written to the store
- [x] **Certificate files** reloaded when they change, served with their whole chain
- [x] **Forwarded headers** (`X-Forwarded-For`, `-Proto`, `-Host`, `X-Real-IP`), and clients found behind trusted proxies
- [x] **Timeouts** toward clients and upstreams, and a graceful stop that waits for open connections

Planned next (see [ADR 0003](adrs/0003-configuration-model.md)):

- [ ] Gateway API resources for Kubernetes


We are constantly adding new features, and we welcome your feedback and contributions. If you have any suggestions or ideas, please feel free to open an issue or a pull request on the GitHub repository.
