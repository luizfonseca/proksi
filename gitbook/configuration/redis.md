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
{% endcode %}

This will then use Redis as backend storage for certificates, challenges and even raw routing configuration. There's a penalty in terms of performance, but it's worth it for the benefits of scalability and reliability.
