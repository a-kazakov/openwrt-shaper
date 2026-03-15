BINARY := slqm

.PHONY: all build clean test lint build-all package

all: build

build:
	cargo build --release
	mkdir -p dist
	cp target/release/$(BINARY) dist/$(BINARY)

# Cross-compilation targets (requires `cross` tool: cargo install cross)
build-arm64:
	cross build --target aarch64-unknown-linux-musl --release
	mkdir -p dist
	cp target/aarch64-unknown-linux-musl/release/$(BINARY) dist/$(BINARY)-arm64

build-armv7:
	cross build --target armv7-unknown-linux-musleabihf --release
	mkdir -p dist
	cp target/armv7-unknown-linux-musleabihf/release/$(BINARY) dist/$(BINARY)-armv7

build-mipsle:
	cross build --target mipsel-unknown-linux-musl --release
	mkdir -p dist
	cp target/mipsel-unknown-linux-musl/release/$(BINARY) dist/$(BINARY)-mipsle

build-all: build-arm64 build-armv7 build-mipsle

test:
	cargo test

test-cover:
	cargo test
	@echo "Note: use cargo-llvm-cov for coverage reports"

lint:
	cargo clippy -- -D warnings

clean:
	cargo clean
	rm -rf dist/ coverage.out coverage.html

# Package for opkg
package: build-all
	./scripts/package.sh
