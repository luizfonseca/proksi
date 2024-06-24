# Environment variables

Proksi can be configured using environment variables and **they will have higher priority over the config file**.&#x20;

They are mapped to the configuration file keys, always start with `PROKSI_` and can be used to override the default values. For nested keys, use the `__` character.



### Example:

For the key `service_name`, the environment variable `PROKSI_SERVICE_NAME` can used

For the key `worker_threads`, the environment variable `PROKSI_WORKER_THREADS` can be used

For the key `logging.level`, the environment variable `PROKSI_LOGGING__LEVEL` can be used (note the `__` separator due to the nested key)

For keys that accept a list of values, e.g. `routes`, the environment variable `PROKSI_ROUTES` can be used with a string value like this:

```bash
export PROKSI_ROUTES='[{host="example.com", upstreams=[{ip="10.0.1.24", port=3001}]'
```

In the future you might be able to use `PROKSI_ROUTES__0__HOST` to set the host of the first route (or any other), but this is not yet implemented.
