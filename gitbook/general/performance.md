# Performance

Early tests are promising, but we need to do more testing to see how Proksi performs under _real_ load. There are also some optimizations that can be done to improve performance in the long term, though the focus is on making it **feature complete first**.

An sample run from the `wrk` benchmark on the simple `/ping` endpoint shows the following results:

```bash
# Apple M1 Pro, 16GB
# Memory Usage: 15MB, CPU Usage: 11%, 4 worker threads
# at 2024-05-16T23:47
# Wrk > 50 connections, 4 threads, 30s duration
wrk -c 50 -t 4 -d 30s http://127.0.0.1/ping

Running 30s test @ http://127.0.0.1/ping
  4 threads and 50 connections
  Thread Stats   Avg      Stdev     Max   +/- Stdev
    Latency   561.57us  218.11us  14.91ms   84.14%
    Req/Sec    21.43k     3.53k   24.72k    79.32%
  2566858 requests in 30.10s, 315.79MB read
Requests/sec:  85274.55
Transfer/sec:     10.49MB
```
