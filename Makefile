lint:
	cargo clippy -- -D clippy::pedantic -D clippy::perf
test:
	cargo test --all-features
build.release:
	cargo build --release
build.dev:
	cargo build
