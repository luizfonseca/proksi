# Docker

Similar to other proxies, Proksi can be run as a Docker container. The following command will run the latest version of it:

```bash
docker run -d -p 80:80 -p 443:443 -v /path/to/config:/etc/proksi/ luizfonseca/proksi
```

If you are using `docker-compose.yml`  to manage your services, you can configure Proksi as your main host-mode container without even creating a `proksi.hcl` file.

```yaml
version: '3.8'
services:
  proksi:
    environment:
      PROKSI_LOGGING__LEVEL: "info"
      PROKSI_WORKER_THREADS: 2

      # Enables Proksi to fetch services/containers 
      # matching Smart labels 
      PROKSI_DOCKER__ENABLED: "true"
      PROKSI_DOCKER__MODE: container

      PROKSI_LETS_ENCRYPT__ENABLED: "true"
      PROKSI_LETS_ENCRYPT__STAGING: "true"
      PROKSI_LETS_ENCRYPT__EMAIL: "contact@email.net"

      PROKSI_PATHS__LETS_ENCRYPT: "/etc/proksi/certs"
    image: luizfonseca/proksi:latest
    networks:
      # Any service in the same network will be able to communicate with Proksi
      - web 
    ports:
      - "80:80"
      - "443:443"
    volumes:
      - /var/run/docker.sock:/var/run/docker.sock
      - /path/to/config:/etc/proksi/certs
```

And then you can expose any service by using `proksi.host` and `proksi.enable` labels. For example a simple `nginxdemos/hello` container/service:

```yaml
services:
  # ... (include Proksi configuration)
  web:
    image: nginxdemos/hello

    networks:
      - public
      - shared
    deploy:
      replicas: 2
    labels:
      proksi.enabled: "true"
      proksi.host: "your-site.localhost"
      proksi.port: "80" # no need to publish host ports

      # If you are running locally
      proksi.ssl_certificate.self_signed_on_failure: "true"
```

