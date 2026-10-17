# Request ID

In order to enable a particular route to include `x-request-id`, you can do the following:

**proksi.hcl**
```hcl
lets_encrypt {
  enabled = true
  email = "test@email.com"
}

host "mywebsite.com" {
  location "/" {
    request_id {}
    upstream "localhost:3000" {}
  }
}
```
