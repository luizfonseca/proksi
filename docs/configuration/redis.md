---
description: Use Redis/Dragonfly to store certificates, and distribute Proksi instances across multiple servers.
---


# Distributed configuration with Redis/Dragonfly

Most use cases require a distributed configuration setup. This can be achieved by using Redis or Dragonfly as a distributed cache for storing certificates and distributing Proksi instances across multiple servers.

## Redis Configuration

To configure Proksi to use Redis as a distributed cache, you can set the following in your `proksi.hcl` file:

{% code title="proksi.hcl" lineNumbers="true" %}
```hcl
store {
  store_type = "redis"
  redis_url = "redis://localhost:6379"
}
```
{% endcode %}

This will then use Redis as backend storage for certificates, challenges and routes, shared by every Proksi instance.

## Routes

Routes can be stored in Redis, in addition to the configuration file and docker labels. Every instance syncs them in the background (they're never read from Redis while handling a request):

{% code title="proksi.hcl" lineNumbers="true" %}
```hcl
store {
  store_type = "redis"
  redis_url = "redis://localhost:6379"

  # How often routes are synced from the store (default: 10)
  sync_interval_secs = 10
}
```
{% endcode %}

- When a host is defined in several places, routes from the store win over docker labels, which win over the configuration file.
- Every change increments a revision. Each instance reports the revision it applied, so you can check that a change reached every instance.
- If Redis can't be reached, the routes from the last sync are kept.

See [the specification](../general/specifications.md#store-keys) for the keys and format.
