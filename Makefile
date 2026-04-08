.PHONY: build build-debug build-release clean test run help install

# Default target
help:
	@echo "swarmctl Makefile"
	@echo ""
	@echo "Available targets:"
	@echo "  build         - Build debug version"
	@echo "  build-release - Build release version (optimized)"
	@echo "  build-docker  - Build using Docker"
	@echo "  clean         - Clean build artifacts"
	@echo "  test          - Run tests"
	@echo "  run           - Run the application"
	@echo "  install       - Install to ~/.local/bin"
	@echo "  uninstall     - Uninstall from ~/.local/bin"
	@echo ""
	@echo "Usage:"
	@echo "  make build-release"

# Build debug version
build:
	cargo build

# Build release version
build-release:
	cargo build --release

# Build using Docker
build-docker:
	docker run --rm -v $$(pwd):/app -w /app rust:latest cargo build --release

# Clean build artifacts
clean:
	cargo clean

# Run tests
test:
	cargo test

# Run the application (debug)
run:
	cargo run --

# Run with arguments
run-args: export
	cargo run -- $(ARGS)

# Install to ~/.local/bin
install:
	@mkdir -p ~/.local/bin
	cp ./target/release/swarmctl ~/.local/bin/
	@echo "Installed to ~/.local/bin/swarmctl"

# Uninstall
uninstall:
	rm -f ~/.local/bin/swarmctl
	@echo "Uninstalled from ~/.local/bin"

# Format code
fmt:
	cargo fmt

# Check code
check:
	cargo check

# Lint code
clippy:
	cargo clippy

# Build and run tests
ci: build-release test
