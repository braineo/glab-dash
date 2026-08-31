.PHONY: all
all: format lint test

.PHONY: format
format:
	cargo fmt --all

.PHONY: lint
lint:
	cargo clippy --workspace --all-targets --all-features --fix --allow-dirty -- -D warnings

.PHONY: test
test:
	cargo test --workspace

.PHONY: install
install:
	cargo build --release
	cargo install --path crates/glab-dash
