TOOL := "jimmystewpot/traceroute"

.PHONY: all build test lint fmt clean

all: fmt lint test build

fmt:
	cargo fmt --check

lint:
	cargo clippy --all-targets -- -D warnings

test:
	cargo test --verbose

build:
	cargo build --release

clean:
	cargo clean
