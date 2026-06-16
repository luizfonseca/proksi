service_name = "mixed-environment"

# Multiple proxies with different SSL configurations
proxies = [
  # Production HTTPS with Let's Encrypt
  {
    host = "0.0.0.0:443"
    ssl = {
      enabled = true
      min_proto = "v1.2"
      max_proto = "v1.3"
      acme = {
        enabled = true
        challenge_port = 80
      }
    }
    routes = [
      {
        host = "production.example.com"
        upstreams = [
          { ip = "10.0.1.100", port = 3000, weight = 2 },
          { ip = "10.0.1.101", port = 3000, weight = 1 }
        ]
      }
    ]
    worker_threads = 6
  },
  
  # Development HTTP-only
  {
    host = "0.0.0.0:8080"
    ssl = null
    routes = [
      {
        host = "dev.example.com"
        upstreams = [
          { ip = "127.0.0.1", port = 3001 }
        ]
        headers = {
          add = [
            { name = "X-Environment", value = "development" }
          ]
        }
      }
    ]
    worker_threads = 1
  },
  
  # Staging HTTPS with custom ACME port
  {
    host = "0.0.0.0:8443"
    ssl = {
      enabled = true
      acme = {
        enabled = true
        challenge_port = 8081
      }
    }
    routes = [
      {
        host = "staging.example.com"
        upstreams = [
          { ip = "10.0.2.100", port = 3000 }
        ]
      }
    ]
    worker_threads = 2
  }
]

logging = {
  level = "info"
  format = "pretty"
  access_logs_enabled = true
}

lets_encrypt = {
  email = "devops@example.com"
  staging = false
  enabled = true
}

docker = {
  enabled = false
}