# Cache

Proksi can be configured to use a cache to improve the performance of your proxy server.

## Cache configuration

The cache configuration is defined in the `routes` section of the Proksi configuration file.

Each route can have a `cache` section that specifies the cache configuration for that route.

The cache configuration includes the following options:

- `enabled`: Whether the cache is enabled for the route. Defaults to `false`.
- `cache_type`: Which cache backend to use. Defaults to `memcache`. Other options are `disk`.
- `expires_in_secs`: The number of seconds the cache should be valid for. Defaults to `360`.
- `stale_if_error_secs`: The number of seconds the cache should be valid for if an error occurs. Defaults to `60`.
- `stale_while_revalidate_secs`: The number of seconds the cache should be valid for if the response is revalidated. Defaults to `60`.
- `path`: The path to the cache directory. Defaults to `/tmp`.

Here's an example of a route with a cache configuration:

```hcl
# proksi.hcl file
routes = [
  {
    host = "example.com",
    cache {
      enabled = true
      cache_type = "memcache"
      expires_in_secs = 360
      stale_if_error_secs = 60
      stale_while_revalidate_secs = 60
      path = "/tmp
    }

    upstreams = [{
      ip =  "10.0.1.3" 
      port =  "3000,
    }]
  }
]
```

## Cache usage

When a request is made to a route with a cache configuration, Proksi will check if the response is already in the cache. If it is, the response will be served from the cache instead of making a new request to the upstream server.

If the response is not in the cache, Proksi will make a new request to the upstream server and cache the response. The cache will be updated with the new response if the response is valid for the configured expiration time.
