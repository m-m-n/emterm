# Increase rustc stack size to avoid SIGSEGV during full release builds
export RUST_MIN_STACK := 67108864

VERSION := $(shell git describe --tags --always --dirty 2>/dev/null | sed 's/^v//' || echo "0.0.0")
# Hash-only (no tags) -> prefix with 0.0.0
ifeq ($(findstring .,$(VERSION)),)
  VERSION := 0.0.0-$(VERSION)
endif

CARGO_TARGET_HOST := src-tauri/target-host
CARGO_TARGET_WIN  := src-tauri/target-win
MANIFEST := --manifest-path src-tauri/Cargo.toml

.PHONY: help setup viewer settings web dev build cli-build win-build dpkg cli-dpkg install clean

help: ## Show this help
	@grep -E '^[a-zA-Z_-]+:.*?## .*$$' $(MAKEFILE_LIST) | awk 'BEGIN {FS = ":.*?## "}; {printf "  \033[36m%-12s\033[0m %s\n", $$1, $$2}'

setup: ## Install required toolchains and bun deps
	rustup target add x86_64-pc-windows-msvc
	cargo install cargo-xwin
	bun install

viewer: ## Build the Markdown viewer web bundle (src-tauri/viewer/dist)
	bun run build:viewer

settings: ## Build the settings window web bundle (src-tauri/settings/dist)
	bun run build:settings

web: viewer settings ## Build both web bundles

dev: web ## Run eMterm (debug build, default GUI feature)
	CARGO_TARGET_DIR=src-tauri/target cargo run $(MANIFEST)

build: web ## Release build (GUI, Linux host)
	@echo "Building version: $(VERSION)"
	CARGO_TARGET_DIR=$(CARGO_TARGET_HOST) cargo build --release $(MANIFEST)

cli-build: ## Release build (CLI only, --no-default-features)
	CARGO_TARGET_DIR=$(CARGO_TARGET_HOST) cargo build --release --no-default-features $(MANIFEST)

win-build: web ## Windows cross-build via cargo-xwin (emterm.exe)
	@echo "Building version: $(VERSION) for Windows"
	CARGO_TARGET_DIR=$(CARGO_TARGET_WIN) cargo xwin build --release --target x86_64-pc-windows-msvc $(MANIFEST)

dpkg: setup web ## Build the GUI deb package (build/emterm_<ver>_<arch>.deb)
	bash scripts/build-dpkg.sh

cli-dpkg: ## Build the CLI-only deb package (build/emterm-cli_<ver>_<arch>.deb)
	EMTERM_CLI_ONLY=1 bash scripts/build-dpkg.sh

install: build dpkg ## Build and install the GUI deb locally
	sudo dpkg -i build/emterm_$(VERSION)_*.deb

clean: ## Clean all build artifacts
	rm -rf build
	rm -rf src-tauri/viewer/dist src-tauri/settings/dist
	cargo clean $(MANIFEST)
