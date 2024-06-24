# Logging

## HCL Configuration

The logging configuration can be set using the `logging` key in the HCL configuration file. The default configuration is:

```hcl
logging {
  level = "info"
  access_logs_enabled = true
  error_logs_enabled = true
  format = "json"
  path = "/tmp"
  rotation = "never"
}
```

You can customize the logging configuration by adding the following keys to the `logging` block:

| Key | Description |
| --- | --- |
| level | The logging level (`debug`, `info`, `warn`, `error`, `trace`) |
| access_logs_enabled | Whether to enable access logs (default: true) |
| error_logs_enabled | Whether to enable error logs (default: false) |
| format | The logging format (`json`, `pretty`) |
| path | The path to the log file (default: /tmp) |
| rotation | The rotation frequency (`daily`, `hourly`, `minutely`, `never`) |

For example, to set the logging level to `debug`, the format to `pretty`, the path to `/var/log/proksi`, and the rotation to `daily`, you can use the following configuration:

```hcl
logging {
  level = "debug"
  format = "pretty"
  path = "/var/log/proksi"
  rotation = "daily"
}
```

This will then create a log file named `proksi.log.<date>` in the `/var/log/proksi` directory and rotate it daily.


## Logging Levels (CLI)

The logging level can be set using the `--log.level` flag. The default level is `info`.

| Level | Description |
| --- | --- |
| debug | Shows debug information |
| info | Shows information about the service |
| warn | Shows warnings |
| error | Shows errors |
| trace | Shows trace information |

## Logging Format

The logging format can be set using the `--log.format` flag. The default format is `json`.

| Format | Description |
| --- | --- |
| json | Logs in JSON format |
| pretty | Logs in a human-readable format |

## Logging Path

The logging path can be set using the `--log.path` flag. The default path is `/tmp`.

If a path is provided, the logs will be written to a file in the specified path. The file name will be prefixed or named `proksi.log.*`.

## Logging Rotation

The logging rotation can be set using the `--log.rotation` flag. The default rotation is `never`.

| Rotation | Description |
| --- | --- |
| daily | Rotates the log file daily |
| hour  | Rotates the log file hourly |
| minutely | Rotates the log file minutely |
| never | Does not rotate the log file |


## Logging Examples

Here are some examples of how to set the logging level, format, path, and rotation:

```bash
# Set the logging level to debug
proksi --log.level debug

# Set the logging format to pretty
proksi --log.format pretty

# Set the logging path to /var/log/proksi
proksi --log.path /var/log/proksi

```

In this example, the logging level is set to `debug`, the format is set to `pretty`, the path is set to `/var/log/proksi`, and the rotation is set to `daily`.
