lint:
	cargo clippy -- -D clippy::pedantic -D clippy::perf -D clippy::complexity -D clippy::style -D clippy::correctness -D clippy::suspicious
lint.fix:
	cargo clippy --fix --allow-dirty --allow-staged
test:
	cargo test --all-features
build.release:
	cargo build --release
build.dev:
	cargo build
