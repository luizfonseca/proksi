---
description: >-
  Adds a `x-request-id` header to every response to downstream (client) and
  upstream (your website)
---

# Request ID

In order to enable a particular route to include `x-request-id`, you can do the following:

{% code title="proksi.hcl" lineNumbers="true" %}
```hcl
lets_encrypt {
  enabled = true
  email = "test@email.com"
}

routes = [
  {
    host = "mywebsite.com"
    
    plugins = [{ 
      name = "request_id" 
    }]
  }
]
```
{% endcode %}
