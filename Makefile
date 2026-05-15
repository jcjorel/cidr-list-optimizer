.PHONY: build test test-stress test-all install clean lint publish

build:
	cargo build --release

test:
	cargo test

test-stress:
	cargo test -p cidr-optimizer --features stress

test-all: test test-stress

install:
	cargo install --path crates/cidr-optimizer-cli

clean:
	cargo clean

lint:
	cargo clippy

publish:
	cargo publish -p cidr-optimizer
	cargo publish -p cidr-optimizer-cli
