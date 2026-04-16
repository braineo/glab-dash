.PHONY: all
all: format lint test

.PHONY: format
format:
	cargo fmt --all

.PHONY: lint
lint:
	cargo clippy --all-targets --all-features --fix --allow-dirty -- -D warnings

.PHONY: test
test:
	cargo test

.PHONY: install
install:
	cargo build --release
	cargo install --path .
