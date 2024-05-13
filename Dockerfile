FROM rust:slim-buster
RUN apt-get update && apt-get install -y libssl-dev pkg-config
WORKDIR /app


RUN rustup target add x86_64-unknown-linux-musl
RUN rustup target add x86_64-unknown-linux-gnu


RUN cargo build --release --target x86_64-unknown-linux-musl
