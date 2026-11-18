# Makefile for alphen-box (Alphen OS ARM64 Container Runtime)

CARGO ?= cargo
PYTHON ?= python
TARGET ?= aarch64-unknown-linux-musl
RELEASE_BIN = target/$(TARGET)/release/alphen-box
LOCAL_BIN = target/release/alphen-box
BUILD_OPTS = --release

.PHONY: all build build-arm64 test size-check bench package clean fmt clippy

all: build

build:
	$(CARGO) build $(BUILD_OPTS)

build-arm64:
	@echo "==> Building for target $(TARGET)"
	$(CARGO) build $(BUILD_OPTS) --target $(TARGET)

test:
	$(CARGO) test --all

fmt:
	$(CARGO) fmt --all -- --check

clippy:
	$(CARGO) clippy --all-targets --all-features -- -D warnings

size-check: build
	$(PYTHON) scripts/size_check.py

bench:
	@echo "==> Running micro-benchmarks"
	$(CARGO) test --release --all -- --nocapture bench

package: build
	@echo "==> Packaging alphen-box for Alphen OS"
	$(PYTHON) scripts/package.py

clean:
	$(CARGO) clean
