# Features

Proksi is evolving day by day, and new features are being added all the time. Below is a list of the features that are currently available in Proksi:

- [x] **HTTPS**: SSL termination, automatic redirect from HTTP and certificate renewal (Let's Encrypt)
- [x] **Plain HTTP** hosts (`tls = false`), for running behind another load balancer or on an IP address
- [x] **Hosts and locations**: the most specific location of a host serves each request (static paths, parameters and wildcards)
- [x] **Catch-all host** (`host "*"`) for names without a host of their own
- [x] **Listen addresses per host**: Proksi only opens the ports its hosts need
- [x] **Docker**: Swarm/Compose Label Support
- [x] **Load Balancing** (✅ Round Robin, ✅ Weighted Round Robin, ⛔︎ Least Connections)
- [X] Health Checks
- [x] **Shared store** (Redis, SQLite, Postgres) to run many Proksi servers with the same routes and certificates
- [x] **Auto reload** of the configuration file, without restarting
- [x] **HCL functions** Extensible through configuration with `env` and `import`
- [x] Header manipulation for DOWNSTREAM & UPSTREAM (add/replace, remove)
- [x] **Caching** in memory or on disk
- [X] **Basic** Authentication
- [X] **Oauth2** Authentication (Google, Facebook, ✅ Github, ✅ WorkOs etc)
- [X] **RequestId** Middleware
- [x] **Rate limiting** per location and client
- [x] **Body limits** per location
- [x] **TCP** proxying: plain, TLS with the host's certificate (chosen by SNI) and TLS passthrough
- [x] Sharing port `443` between HTTPS and TCP, chosen by name (SNI) or protocol (ALPN), with TLS terminated or passed through
- [x] **Kubernetes** ingress controller: `Ingress` resources served with their ready pods as upstreams, TLS secrets, plugins through annotations, status and events, with a Helm chart
- [x] **Metrics** in the Prometheus format, per host, location and upstream, on an admin listener with a token

Planned next (see [ADR 0003](adrs/0003-configuration-model.md)):

- [ ] Gateway API resources for Kubernetes


We are constantly adding new features, and we welcome your feedback and contributions. If you have any suggestions or ideas, please feel free to open an issue or a pull request on the GitHub repository.
