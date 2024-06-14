lint:
	cargo clippy -- -D clippy::pedantic -D clippy::perf
lint.fix:
	cargo clippy --fix --allow-dirty --allow-staged
test:
	cargo test --all-features
build.release:
	cargo build --release
build.dev:
	cargo build
