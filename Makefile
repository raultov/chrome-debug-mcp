.PHONY: all fmt fmt-check clippy test dupes dupes-cleanup publish-check version-check check build install help

# Default target: run all mandatory quality gates
all: check

# Auto-fix code formatting
fmt:
	cargo fmt

# Verify code formatting
fmt-check:
	cargo fmt -- --check

# Run linter checks
clippy:
	cargo clippy --all-targets --all-features -- -D warnings

# Run unit tests
test:
	cargo test --all-targets --all-features

# Check for code duplication (requires cargo-dupes)
dupes:
	@test -x "$$(command -v cargo-dupes)" || (echo "cargo-dupes not found. Installing..." && cargo install cargo-dupes --version 0.2.1 --locked)
	cargo dupes check

# Show stale duplication suppressions
dupes-cleanup:
	@test -x "$$(command -v cargo-dupes)" || (echo "cargo-dupes not found. Installing..." && cargo install cargo-dupes --version 0.2.1 --locked)
	cargo dupes cleanup --dry-run

# Verify the crate packages cleanly without local path overrides or missing dependencies
publish-check:
	cargo publish --dry-run --allow-dirty

# Verify package.json version matches Cargo.toml (npm/Glama metadata)
version-check:
	@cargo_v=$$(awk -F'"' '/^version/ {print $$2; exit}' Cargo.toml); \
	npm_v=$$(awk -F'"' '/"version"/ {print $$4; exit}' package.json); \
	if [ -n "$$cargo_v" ] && [ "$$cargo_v" = "$$npm_v" ]; then \
		echo "Version in sync: $$cargo_v"; \
	else \
		printf 'Version mismatch: Cargo.toml=%s package.json=%s\n' "$$cargo_v" "$$npm_v"; \
		exit 1; \
	fi

# Run all local quality gates sequentially (fmt, clippy, unit tests, dupes, publish-check, version-check)
check: fmt-check clippy test dupes publish-check version-check

# Build release binary
build:
	cargo build --release --all-features

# Detect OS binary extension
ifeq ($(OS),Windows_NT)
    EXT := .exe
else
    EXT :=
endif

# Install release binary to CARGO_HOME/bin (defaults to ~/.cargo/bin)
CARGO_HOME ?= $(HOME)/.cargo
DESTDIR ?= $(CARGO_HOME)/bin

install: build
	@mkdir -p "$(DESTDIR)"
	@cp -f target/release/chrome-debug-mcp$(EXT) "$(DESTDIR)/chrome-debug-mcp$(EXT)"
	@echo "Installed binary to $(DESTDIR):"
	@echo "  - chrome-debug-mcp$(EXT)"

# Display available make targets
help:
	@echo "Available targets:"
	@echo "  make (or make all) - Default target: alias for 'make check'"
	@echo "  make check         - Run all local quality gates (fmt-check, clippy, test, dupes, publish-check, version-check)"
	@echo "  make fmt           - Auto-fix code formatting with cargo fmt"
	@echo "  make fmt-check     - Verify code formatting with cargo fmt -- --check"
	@echo "  make clippy        - Run linter checks with cargo clippy"
	@echo "  make test          - Run unit tests"
	@echo "  make dupes         - Run code duplication check with cargo-dupes (auto-installs if missing)"
	@echo "  make dupes-cleanup - Show stale duplication suppressions"
	@echo "  make publish-check - Verify cargo publish packaging dry-run"
	@echo "  make version-check - Verify package.json version matches Cargo.toml"
	@echo "  make build         - Build release binary"
	@echo "  make install       - Build release binary and copy it to ~/.cargo/bin"
