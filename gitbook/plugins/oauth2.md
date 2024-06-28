---
description: Protects a route using a provider and the Oauth2 protocol
---

# OAuth2

This plugin protects a given route by authenticating against a provider and a JWT token sent as an HTTP-only cookie for you particular domain.



### Providers

* `github`
* `workos`



### Options

Plugin options are always passed via the `config` key.

<table><thead><tr><th width="310"></th><th></th></tr></thead><tbody><tr><td><code>provider</code></td><td>One of the providers listed above</td></tr><tr><td><code>client_id</code> </td><td>Client ID of your app in the provider of your choosing</td></tr><tr><td><code>client_secret</code></td><td>Client Secret of your app in the provider of your choosing</td></tr><tr><td><code>jwt_secret</code></td><td>The secret for the JWT token used in the generate HTTP-only cookie. Needs to be at least 64 chars.</td></tr><tr><td><code>validations</code></td><td>An list (array) of validations for your provider</td></tr></tbody></table>



### Validations

The OAuth2 plugins allows you to define whether a given user can access the domain requested.



#### Email

To only allow access from specific emails:

```hcl
# ... the rest of the plugin config from above
validations  = [
  { key = "email", values = ["email@gmail.com", "valid@yahoo.com"]
]
```

#### Username&#x20;

To only allow access from specific usernames (depends on provider)

```hcl
validations  = [
  { key = "username", values = ["user2021", "proksi"]
]
```

#### Combined

You can combine all validations together

```hcl
validations  = [
  { key = "username", values = ["user2021", "proksi"],
  { key = "email", values = ["email@gmail.com", "valid@yahoo.com"]
]
```



### Usage

A complete plugin definition looks like the following:

{% code title="proksi.hcl" lineNumbers="true" %}
```hcl
lets_encrypt {
    enabled = true
    email = "test@email.com"
}

routes = [{
    host = "website.com"
    
    upstreams = [{ ip = "localhost", port = 3000 }]
    
    plugins = [
        { name = "request_id" },
        
        { 
            name = "oauth2", 
            config = { 
                provider = "github"
                client_id = "lv1.98asd7h12h3"
                client_secret = "lvl2.91823hl1238d"
                # Generated using `openssl rand -hex 64`
                jwt_secret = "d1a86503f928b387dcde695176e02c9c6fb0a96f91f4436d2f724b312c4a1e7fc16d5f86bd37f4fe6267e628dca8a55f621f8e4f2f41725ff00cdfbb971b0384"
                validations = [
                    { key = "email", values = ["me@proksi.info"] }
                ]
            } 
        }
    ]
}]
```
{% endcode %}





