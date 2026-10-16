FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y ca-certificates && apt-get clean

# A static (musl) binary, so it runs regardless of the image's glibc
COPY proksi /app/proksi

WORKDIR /app

EXPOSE 80 443

ENTRYPOINT ["/app/proksi"]
