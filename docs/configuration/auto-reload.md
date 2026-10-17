# Auto Reload

Proksi can be configured to automatically reload the configuration file when it changes. This can be useful when you want to change the configuration without restarting the service.

To enable auto reload, you can set the `auto_reload` key to `true` in the configuration file. The default value is `false`.

{% code title="proksi.hcl" overflow="wrap" lineNumbers="true" %}
```hcl
auto_reload {
  # Whether to enable auto reload (default: false)
  enabled = true
  # The interval (in seconds) to check for changes (default: 30)
  interval_secs = 5

  # extra paths to watch for changes (default: [])
  # This is useful if you are dealing with `import` in the configuration file
  # changes on those imports will trigger a reload on the main configuration
  # file and down.
  # Only .hcl, .yml and .yaml files are considered.
  paths = ["/etc/sites"]
}
```
{% endcode %}

When the configuration file changes:

- if it's invalid, the change is ignored (and logged): Proksi keeps running with the current configuration;
- if only hosts (`host` blocks or `routes`) changed, they are applied without restarting;
- if other settings changed, a new Proksi worker starts with the new configuration and takes over, without dropping connections on Linux (see [Restarts and upgrades](configuration/upgrades.md)).
