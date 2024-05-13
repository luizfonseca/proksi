cargo install cross --git https://github.com/cross-rs/cross

cross build --release --target=x86_64-unknown-linux-gnu
cross build --release --target=x86_64-apple-darwin

mkdir -p dist/proksi_darwin_amd64_v1
mkdir -p dist/proksi_linux_amd64_v1

cp target/x86_64-apple-darwin/release/proksi dist/proksi_darwin_amd64
cp target/x86_64-unknown-linux-gnu/release/proksi dist/proksi_linux_amd64_v1
