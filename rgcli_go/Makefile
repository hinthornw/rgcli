.PHONY: all build test lint format clean install

# Binary name
BINARY=lsc

# Go parameters
GOCMD=go
GOBUILD=$(GOCMD) build
GOTEST=$(GOCMD) test
GOFMT=$(GOCMD) fmt
GOVET=$(GOCMD) vet
GOMOD=$(GOCMD) mod

all: format lint build test

## build: Build the binary
build:
	$(GOBUILD) -o $(BINARY) .

## test: Run tests
test:
	$(GOTEST) -v ./...

## lint: Run linters
lint: vet
	@which golangci-lint > /dev/null || (echo "Installing golangci-lint..." && go install github.com/golangci/golangci-lint/cmd/golangci-lint@latest)
	golangci-lint run ./...

## vet: Run go vet
vet:
	$(GOVET) ./...

## format: Format code
format:
	$(GOFMT) ./...

## clean: Clean build artifacts
clean:
	rm -f $(BINARY)
	rm -f ~/.lsc/config.yaml

## install: Install binary to GOPATH/bin
install: build
	cp $(BINARY) $(GOPATH)/bin/

## deps: Download dependencies
deps:
	$(GOMOD) download
	$(GOMOD) tidy

## run: Build and run
run: build
	./$(BINARY)

## help: Show this help
help:
	@echo "Usage: make [target]"
	@echo ""
	@echo "Targets:"
	@grep -E '^## ' $(MAKEFILE_LIST) | sed 's/## /  /'
