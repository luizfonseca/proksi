FROM debian:buster-slim
RUN apt-get update && apt-get install -y ca-certificates && apt-get clean

COPY proksi /app/proksi

WORKDIR /app

EXPOSE 80 443

ENTRYPOINT ["/app/proksi"]
