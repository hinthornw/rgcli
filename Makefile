.PHONY: all build test lint format clean install

BINARY=ailsd

all: format lint build test

## build: Build the binary (release)
build:
	cargo build --release

## test: Run tests
test:
	cargo test

## lint: Run clippy
lint:
	cargo clippy --all-targets -- -D warnings

## format: Format code
format:
	cargo fmt

## clean: Clean build artifacts and config
clean:
	rm -rf target
	rm -f ~/.ailsd/config.yaml

## install: Install binary to ~/.cargo/bin
install:
	cargo install --path .

## run: Build and run
run: build
	./target/release/$(BINARY)

## help: Show this help
help:
	@echo "Usage: make [target]"
	@echo ""
	@echo "Targets:"
	@grep -E '^## ' $(MAKEFILE_LIST) | sed 's/## /  /'
