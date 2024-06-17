FROM debian:bullseye-slim as builder
RUN apt-get update && apt-get install -y ca-certificates && apt-get clean

# ---
FROM scratch
COPY --from=builder /etc/ssl/certs/ca-certificates.crt /etc/ssl/certs/

COPY proksi /app/proksi

WORKDIR /app

EXPOSE 80 443

ENTRYPOINT ["/app/proksi"]
