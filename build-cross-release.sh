docker run --rm -v `pwd`:/app rust:slim-buster sh -c 'cd /app && cargo build --release --target=x86_64-unknown-linux-gnu'
cargo build --release --target=x86_64-apple-darwin

mkdir -p dist/darwin_amd64 && cp target/x86_64-apple-darwin/release/proksi dist/darwin_amd64
mkdir -p dist/linux_amd64 && cp target/x86_64-unknown-linux-gnu/release/proksi dist/linux_amd64
