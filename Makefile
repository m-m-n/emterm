VERSION := $(shell git describe --tags --always --dirty 2>/dev/null | sed 's/^v//' || echo "0.0.0")
# Hash-only (no tags) -> prefix with 0.0.0
ifeq ($(findstring .,$(VERSION)),)
  VERSION := 0.0.0-$(VERSION)
endif

.PHONY: dev build win-build dpkg install clean help setup

help: ## Show this help
	@grep -E '^[a-zA-Z_-]+:.*?## .*$$' $(MAKEFILE_LIST) | awk 'BEGIN {FS = ":.*?## "}; {printf "  \033[36m%-10s\033[0m %s\n", $$1, $$2}'

setup: ## Install required tools and dependencies
	rustup target add wasm32-unknown-unknown
	cargo install wasm-pack
	bun install

dev: ## Run in development mode
	bun tauri dev

build: ## Build release (deb/rpm/nsis) with git version
	@echo "Building version: $(VERSION)"
	bun tauri build --config '{"version":"$(VERSION)"}'

win-build: ## Build Windows release (cross-compile with cargo-xwin)
	@echo "Building version: $(VERSION) for Windows"
	bun tauri build --runner cargo-xwin --target x86_64-pc-windows-msvc --config '{"version":"$(VERSION)"}'

dpkg: setup ## Build custom dpkg package
	bash scripts/build-dpkg.sh

install: build ## Build and install deb package
	sudo dpkg -i src-tauri/target/release/bundle/deb/emterm_$(VERSION)_*.deb

clean: ## Clean build artifacts
	bun run clean
	cargo clean --manifest-path src-tauri/Cargo.toml
