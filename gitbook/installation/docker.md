# Docker

Similar to other proxies, Proksi can be run as a Docker container. The following command will run the latest version of it:

```bash
docker run -d -p 80:80 -p 443:443 -v /path/to/config:/etc/proksi/ luizfonseca/proksi
```

If you are using `docker-compose.yml`  to manage your services, you can configure Proksi as your main host-mode container:

```yaml
version: '3.8'
services:
  proksi:
    image: luizfonseca/proksi:latest
    command: --config-path /etc/proksi/ # Proksi will try to find proksi.toml/yaml
    network:
      # Any service in the same network will be able to communicate with Proksi
      - web 
    ports:
      - "80:80"
      - "443:443"
    volumes:
      - /path/to/config:/etc/proksi
```



