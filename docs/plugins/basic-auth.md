---
description: Protects a route by using an authorization header
---

# Basic Auth

{% hint style="info" %}
This is not necessarily a very safe method as anyone can try to brute force the username and password.
{% endhint %}

By enabling this, routes can only be accessed if the user (downstream) provides the user & password combination through the `Authorization` header.&#x20;

In many cases, your browser will prompt for this information.



## Options

Plugin options are always passed via the `config` key.

<table><thead><tr><th width="205">Name</th><th>Description</th></tr></thead><tbody><tr><td><code>user</code></td><td>username for the basic authentication</td></tr><tr><td><code>pass</code></td><td>password for the basic authentication</td></tr></tbody></table>



### Usage

{% code title="proksi.hcl" overflow="wrap" lineNumbers="true" %}
```hcl
lets_encrypt {
  enabled = true
  email = "test@email.com"
}

routes = [
 {
   host = "mywebsite.com"
   upstreams = [{ ip = "localhost", port = 3000 }]
   
   plugins = [{
     name = "basic_auth"
     config = {
       user = "91hdjashd1y2u"
       pass = "$17238a81hhasbzh1230%"
     }
   }]
 }

]
```
{% endcode %}
