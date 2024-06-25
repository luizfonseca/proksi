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


### Full list

Below you can find a full list of the configuration keys and their corresponding environment variables.

| Key | Environment variable | Description |
| :--- | :--- | :--- |
| `service_name` | `PROKSI_SERVICE_NAME` | The name of the service |
| `worker_threads` | `PROKSI_WORKER_THREADS` | The number of worker threads |
| `daemon` | `PROKSI_DAEMON` | Whether the service should run as a daemon |
| `logging.level` | `PROKSI_LOGGING__LEVEL` | The log level |
| `logging.format` | `PROKSI_LOGGING__FORMAT` | The log format |
| `logging.path` | `PROKSI_LOGGING__PATH` | The path where we should write logs files |
| `logging.rotation` | `PROKSI_LOGGING__ROTATION` | The rotation policy of the log files |
| `lets_encrypt.enabled` | `PROKSI_LETS_ENCRYPT__ENABLED` | Whether lets encrypt should be enabled |
| `lets_encrypt.email` | `PROKSI_LETS_ENCRYPT__EMAIL` | The email address used for lets encrypt |
| `lets_encrypt.staging` | `PROKSI_LETS_ENCRYPT__STAGING` | Whether lets encrypt should be used in staging mode |
| `paths.lets_encrypt` | `PROKSI_PATHS__LETS_ENCRYPT` | The path where we should write the lets encrypt certificates |
| `docker.enabled` | `PROKSI_DOCKER__ENABLED` | Whether the docker service should be enabled |
| `docker.interval_secs` | `PROKSI_DOCKER__INTERVAL_SECS` | The interval (in seconds) to check for label updates |
| `docker.endpoint` | `PROKSI_DOCKER__ENDPOINT` | The docker endpoint to connect to the docker socket/api |
