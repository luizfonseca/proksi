docker run --rm -v`pwd`:/app liuchong/rustup:nightly sh -c 'cd /app && cargo build --release --target=x86_64-unknown-linux-gnu'
cargo build --release --target=x86_64-apple-darwin

mkdir -p dist/darwin_amd64 && cp target/x86_64-apple-darwin/release/cep dist/darwin_amd64
mkdir -p dist/linux_amd64 && cp target/x86_64-unknown-linux-gnu/release/cep dist/linux_amd64
