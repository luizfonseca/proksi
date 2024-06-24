# 🚀 Performance

Early tests are promising, but there are many variants on how Proksi performs under _real_ load.&#x20;

There are also some optimizations that can be done to improve performance in the long term, though the focus is on making it **feature complete first**.



### Wrk benchmark

An sample run from the `wrk` benchmark on the simple `/ping` endpoint shows the following results:

```bash
# Apple M1 Pro, 16GB
# Memory Usage: 15.2MB, CPU Usage: 13%, 1 worker thread
# at 2024-06-13
# Wrk > 50 connections, 4 threads, 30s duration
wrk -c 50 -t 4 -d 30s http://127.0.0.1/ping

Running 30s test @ http://127.0.0.1/ping
  4 threads and 50 connections
  Thread Stats   Avg      Stdev     Max   +/- Stdev
    Latency   376.73us  124.78us   7.83ms   91.64%
    Req/Sec    31.76k     3.03k   34.29k    92.44%
  Latency Distribution
     50%  373.00us
     75%  404.00us
     90%  442.00us
     99%  675.00us
  3803987 requests in 30.10s, 467.98MB read
Requests/sec: 126377.09
Transfer/sec:     15.55MB
```



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

<figure><img src="../.gitbook/assets/one_minute_test.png" alt=""><figcaption></figcaption></figure>
