# Docker swarm

Proksi can be used in conjunction with Docker to automatically discover **services** and route traffic to them.&#x20;

To enable this, you need to add labels to your Docker **services** (_swarm_ or not). The following labels are supported:

```yaml
# docker-compose.yml
version: 3.8
services:
  my_service:
    image: your-service:latest
    ports:
      - "3000:3000"
    deploy:
      labels:
        # Whether the service should be proxied or not.
        # By default, Proksi won't discover any services where the value is not explicitly `true`
        proksi.enable: "true"

        # The hostname that the service should be available at. E.g. `example.com`.
        proksi.host: "example.com"

        # The port that your service is running on. E.g. `3000`.
        proksi.port: "3000"

        # (Optional) The path prefix that the service should be available at.
        # E.g. `/api` will match only requests with "example.com/api*" to this service.
        proksi.path.prefix: "/api"

        # (Optional) The suffix that the service will use to handle requests.
        # E.g. `.json` will match only requests with "example.com/*.json"
        proksi.path.suffix: ".json"

        # (Optional) A dictionary of Headers to add to the response at the end of proxying
        proksi.headers.add: |
          [
            {name="X-Forwarded-For", value="my-api"},
            {name="X-Api-Version", value="1.0\"}
          ]

        # A list of comma-separated headers to remove from the response at the end of proxying.
        proksi.headers.remove: "Server,X-User-Id"
```
