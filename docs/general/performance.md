# Performance

Proksi is built on [Cloudflare Pingora](https://github.com/cloudflare/pingora), and most of the work of each request (parsing, connection pooling, I/O) happens there. Proksi adds the routing, plugins and logging on top of it, which costs around 2µs per request.

### Compared to nginx and Traefik

All proxies forward plain HTTP and HTTPS requests to the same nginx upstream, with the same certificate (RSA 2048), access logs disabled and keep-alive connections to the upstream. The load comes from [oha](https://github.com/hatoo/oha) with 50 connections, on the same machine.

```bash
# Linux (arm64, 8 cores), at 2026-10-16
# nginx 1.26.3, Traefik 3.7.15, Proksi (main, with mimalloc)
# Threads: worker_threads (Proksi), worker_processes (nginx), GOMAXPROCS (Traefik)
```

**1 thread**

| | Requests/s (HTTP / HTTPS) | CPU per request (HTTP / HTTPS) | Latency p99 (HTTP) | Memory (idle → under load) |
| --- | ---: | ---: | ---: | ---: |
| nginx | 112k / 84k | 8.9 / 11.9 µs | 0.65 ms | 15 → 15 MB |
| **Proksi** | 57k / 45k | 17.6 / 22 µs | 1.35 ms | 37 → 43 MB |
| Traefik | 34k / 33k | 29.5 / 30.5 µs | 4.6 ms | 89 → 111 MB |

**4 threads**

| | Requests/s (HTTP / HTTPS) | CPU per request (HTTP / HTTPS) | Latency p99 (HTTP) | Memory (idle → under load) |
| --- | ---: | ---: | ---: | ---: |
| nginx | 398k / 327k | 8.5 / 10.9 µs | 0.37 ms | 29 → 31 MB |
| **Proksi** | 144k / 129k | 26.4 / 29.6 µs | 0.9 ms | 39 → 49 MB |
| Traefik | 109k / 106k | 35.4 / 36.3 µs | 2.2 ms | 91 → 119 MB |

On Linux, Proksi commits its memory upfront ([mimalloc](https://github.com/microsoft/mimalloc)), which is ~15MB of the numbers above. Set `MIMALLOC_ARENA_EAGER_COMMIT=0` to commit it as it's used instead (the default in the Docker image), at the same speed.

These numbers come from a single machine, where the proxy, the upstream and the load generator share the same CPUs. Treat them as a comparison, not as the throughput you will get in production.

### Every pull request

Every pull request is benchmarked against `main` on the same GitHub runner (requests per second, latency, CPU per request and peak memory, for plain HTTP and HTTPS). The results are posted as a comment on the pull request, so changes in performance are visible before they are merged. See [repository](contributing/repository.md#performance-checks) to run it locally.



### CDN performance

Using the following configuration we can see that Proksi holds its own in regards to performance.

```hcl
cache {
  enabled = true
  path = "/tmp"
  cache_type = "disk"
}
```

1 minute test using 4000 concurrent clients.

<figure><img src="/assets/one_minute_test.png" alt=""><figcaption></figcaption></figure>
