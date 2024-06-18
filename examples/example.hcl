service_name = "proksi"
worker_threads = 4

logging {
  level = "INFO"
  access_logs_enabled = true
  error_logs_enabled = false
}

lets_encrypt {
  enabled = true
  email = "your-email@example.com"
  staging = true
}

paths {
  lets_encrypt = "/etc/proksi/letsencrypt"
}

routes = [
  route_file("domain.com.hcl"),
  route_file("domain2.com.hcl"),
]
