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

## install: Install binary to ~/.cargo/bin
install:
	cargo install --path .

## run: Build and run
run: build
	./target/release/$(BINARY)

## check-version: Verify Cargo.toml version has a matching git tag
check-version:
	@cargo_ver=$$(grep '^version' Cargo.toml | head -1 | sed 's/.*"\(.*\)"/\1/'); \
	if git rev-parse "v$$cargo_ver" >/dev/null 2>&1; then \
		echo "Tag v$$cargo_ver already exists — bump version in Cargo.toml first"; \
		exit 1; \
	fi; \
	echo "Version $$cargo_ver is available for tagging"

## release: Bump version, tag, and push (usage: make release)
release: all check-version
	@cargo_ver=$$(grep '^version' Cargo.toml | head -1 | sed 's/.*"\(.*\)"/\1/'); \
	echo "Releasing v$$cargo_ver..."; \
	git tag "v$$cargo_ver" && \
	git push origin main --tags && \
	echo "Released v$$cargo_ver"

## help: Show this help
help:
	@echo "Usage: make [target]"
	@echo ""
	@echo "Targets:"
	@grep -E '^## ' $(MAKEFILE_LIST) | sed 's/## /  /'
