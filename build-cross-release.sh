# rustup target add x86_64-apple-darwin
# rustup target add x86_64-unknown-linux-gnu
# rustup target add aarch64-apple-darwin

docker run --rm -v `pwd`:/app rust:slim-buster sh -c 'cd /app rustup target x86_64-unknown-linux-gnu && cargo build --release --target=x86_64-unknown-linux-gnu --bin proksi'
cargo build --release --target=aarch64-apple-darwin --bin proksi

mkdir -p dist/darwin_amd64 && cp target/x86_64-apple-darwin/release/proksi dist/darwin_amd64
mkdir -p dist/linux_amd64 && cp target/x86_64-unknown-linux-gnu/release/proksi dist/linux_amd64
