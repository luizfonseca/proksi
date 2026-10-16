FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y ca-certificates && apt-get clean

# A static (musl) binary, so it runs regardless of the image's glibc
COPY proksi /app/proksi

WORKDIR /app

# mimalloc commits its memory as it's used instead of upfront (~15MB less, same speed)
ENV MIMALLOC_ARENA_EAGER_COMMIT=0

EXPOSE 80 443

ENTRYPOINT ["/app/proksi"]
