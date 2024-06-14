FROM alpine:3.20.0
RUN apk update && apk add ca-certificates && apk cache clean

COPY proksi /app/proksi

WORKDIR /app

EXPOSE 80 443

ENTRYPOINT ["/bin/sh", "-c", "/app/proksi"]
