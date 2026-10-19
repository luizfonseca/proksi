lint:
	cargo clippy --all-features -- -D clippy::pedantic -D clippy::perf -D clippy::complexity -D clippy::style -D clippy::correctness -D clippy::suspicious -D clippy::unwrap_used -D clippy::expect_used -D clippy::indexing_slicing
lint.fix:
	cargo clippy --fix --allow-dirty --allow-staged
test:
	cargo test --all-features --locked
build.release:
	cargo build --release
build.dev:
	cargo build
dev:
	cargo watch -c -x run -d 1 -i data -i dist
