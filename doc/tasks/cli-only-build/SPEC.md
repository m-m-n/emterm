# Feature: CLI-Only Build

## Overview

Enable building emterm CLI commands (`emterm image`, `emterm markdown`) on headless servers without GUI library dependencies. Uses Cargo feature flags gated by the `EMTERM_CLI_ONLY` environment variable via Makefile/build script.

## Objectives

- Build `emterm image` and `emterm markdown` on servers without `gdk-sys`, `libwebkit2gtk`, etc.
- Provide `EMTERM_CLI_ONLY=1 make dpkg` workflow for CLI-only dpkg package generation
- Maintain full backward compatibility for default GUI builds

## User Stories

### US1: CLI-Only Build on Headless Server
As a developer, I want to build emterm CLI commands on a headless server, so that I can install them for use over SSH.

**Acceptance Criteria:**
- [ ] `EMTERM_CLI_ONLY=1 make dpkg` succeeds without GUI libraries installed
- [ ] Generated binary includes `emterm image` and `emterm markdown` subcommands
- [ ] Generated dpkg has no GUI-related dependencies

### US2: Backward-Compatible GUI Build
As a developer, I want `make dpkg` (without env var) to produce the same GUI package as before, so that nothing breaks.

**Acceptance Criteria:**
- [ ] `make dpkg` produces identical output to current behavior
- [ ] All existing tests pass without modification

## Technical Requirements

### Functional Requirements
- **FR1:** Add `gui` default feature to `Cargo.toml` gating GUI-specific dependencies as optional
- **FR2:** Gate GUI-only top-level modules in `lib.rs` with `#[cfg(feature = "gui")]`. Modules that expose both CLI-reachable items (consumed by `mux` or CLI commands) and GUI-only items gate at the submodule / item level instead.
- **FR3:** Gate `tauri_build::build()` in `build.rs` with `#[cfg(feature = "gui")]`
- **FR4:** Gate `app_lib::run()` in `main.rs` with `#[cfg(feature = "gui")]`
- **FR5:** Gate GUI-only submodules in `commands/mod.rs` (`config`, `font`, `editor`)
- **FR6:** Modify `build-dpkg.sh` to detect `EMTERM_CLI_ONLY` and adjust build and packaging
- **FR7:** Show a user-friendly message when CLI-only binary is run without subcommands

### Non-Functional Requirements
- **NFR1 - Backward Compatibility:** Default build (no env var) must produce identical results
- **NFR2 - Minimal Invasiveness:** `#[cfg]` gates are concentrated at module boundaries when feasible. Modules with mixed CLI/GUI surfaces (currently `pty`) gate at the submodule and item level so the CLI-reachable surface stays exported.

## Implementation Approach

### Architecture

**Build Flow:**
```
Environment Variable             Cargo Feature Flag
─────────────────               ──────────────────
EMTERM_CLI_ONLY unset  ────→    default = ["gui"]  ────→  Full Tauri build
EMTERM_CLI_ONLY=1      ────→    --no-default-features ──→  CLI-only build
```

**Module Gating in lib.rs:**
```
lib.rs
├── #[cfg(feature = "gui")]  pub mod ansi;
├── #[cfg(feature = "gui")]  pub mod image;
├── #[cfg(feature = "gui")]  pub mod logging;
├── #[cfg(feature = "gui")]  mod app;
├── #[cfg(feature = "gui")]  pub mod download_registry;
├── #[cfg(feature = "gui")]  pub mod payloads;
├── #[cfg(feature = "gui")]  pub mod reader;
├── #[cfg(feature = "gui")]  pub mod state;
├── #[cfg(feature = "gui")]  pub mod tauri_commands;
├── pub mod mux;               // always available (CLI daemon path)
├── pub mod pty;               // always available with internal gating (see pty/mod.rs)
├── pub mod commands;          // always available
├── pub mod encoding;          // always available
├── pub mod error;             // always available
├── pub mod protocols;         // always available
├── pub mod sftp;              // always available
├── pub mod ssh;               // always available
├── pub mod validation;        // always available
└── pub mod wsl;               // always available

pty/mod.rs (mixed CLI/GUI surface)
├── pub mod passthrough_scanner;          // always available (used by mux)
├── pub mod visibility;                   // always available; only RawPassthroughBuffer
│                                         // and HIDDEN_PASSTHROUGH_CAPACITY_* are
│                                         // re-exported in CLI builds
├── pub fn generate_session_id;           // always available
├── pub type SessionId / pub enum PtyError; // always available
├── #[cfg(feature = "gui")]  pub mod backpressure;
├── #[cfg(feature = "gui")]  pub mod device_query_scanner;
├── #[cfg(feature = "gui")]  pub mod graceful_shutdown;
├── #[cfg(feature = "gui")]  pub mod kitty_scanner;
├── #[cfg(feature = "gui")]  pub mod manager;
├── #[cfg(feature = "gui")]  pub mod session;
├── #[cfg(feature = "gui")]  pub mod shell;
└── #[cfg(feature = "gui")]  pub mod writer;

commands/mod.rs
├── #[cfg(feature = "gui")]  pub mod config;
├── #[cfg(feature = "gui")]  pub mod font;
├── #[cfg(feature = "gui")]  pub mod editor;
├── pub mod image;             // CLI command
├── pub mod markdown;          // CLI command
└── pub mod tmux;              // shared utility
```

### Data Flow

```
EMTERM_CLI_ONLY=1 make dpkg
  → build-dpkg.sh detects env var
  → cargo build --release --no-default-features
  → Binary with CLI commands only
  → dpkg without .desktop, icons, GUI deps
```

### Dependencies

**Cargo.toml Changes:**

`portable-pty`, `tokio`, and `futures` are always-on because the `mux` daemon path
(non-`gui`) consumes them.

```toml
[features]
default = ["gui"]
gui = [
  "tauri",
  "tauri-plugin-clipboard-manager",
  "tauri-plugin-dialog",
  "tauri-plugin-notification",
  "tauri-plugin-shell",
  "font-kit",
  "tauri-build",
  "raw-window-handle",
  "windows",
]

[dependencies]
# Always-on (CLI + GUI + mux daemon)
clap = { version = "4.5.54", features = ["derive"] }
serde = { version = "1.0", features = ["derive"] }
serde_json = "1.0"
log = { version = "0.4", features = ["std"] }
rust-i18n = "3"
sys-locale = "0.3"
uuid = { version = "1", features = ["v4"] }
base64 = "0.22"
image = { version = "0.25.9", default-features = false, features = ["png", "jpeg", "gif", "webp"] }
png = "0.17"
gif = "0.13"
flate2 = "1"
thiserror = "2"
anyhow = "1"
portable-pty = "0.8"
tokio = { version = "1", features = ["sync", "rt-multi-thread", "macros", "time", "net", "io-util", "io-std", "signal", "process"] }
futures = "0.3"
vt100 = "0.15"

# GUI-only (optional)
tauri = { version = "2.9.5", features = [], optional = true }
tauri-plugin-clipboard-manager = { version = "2", optional = true }
tauri-plugin-dialog = { version = "2", optional = true }
tauri-plugin-notification = { version = "2", optional = true }
tauri-plugin-shell = { version = "2", optional = true }
font-kit = { version = "0.14", optional = true }

[build-dependencies]
tauri-build = { version = "2.5.3", features = [], optional = true }
```

### File Structure

```
src-tauri/
├── Cargo.toml           # Add [features] section, make GUI deps optional
├── build.rs             # Gate tauri_build::build() with cfg
└── src/
    ├── main.rs          # Gate app_lib::run() with cfg
    ├── lib.rs           # Gate GUI-only top-level modules with cfg
    ├── pty/
    │   └── mod.rs       # Internally gate GUI-only submodules; keep
    │                    # passthrough_scanner / visibility::RawPassthroughBuffer
    │                    # / capacity constants always available for mux
    └── commands/
        └── mod.rs       # Gate config, font, editor with cfg

scripts/
└── build-dpkg.sh        # Detect EMTERM_CLI_ONLY, adjust build + packaging
```

### build.rs Changes

```rust
fn main() {
    let version = git_version();
    println!("cargo::rustc-env=APP_VERSION={version}");
    println!("cargo::rerun-if-changed=../.git/HEAD");
    println!("cargo::rerun-if-changed=../.git/refs/tags");
    println!("cargo::rerun-if-changed=../.git/refs/heads");

    #[cfg(feature = "gui")]
    tauri_build::build()
}
```

### main.rs Changes (no-subcommand case)

```rust
_ => {
    #[cfg(feature = "gui")]
    {
        #[cfg(not(test))]
        app_lib::run();
    }
    #[cfg(not(feature = "gui"))]
    {
        // CLI-only build: show help when no subcommand provided
        build_cli().print_help().ok();
        std::process::exit(0);
    }
}
```

### build-dpkg.sh Changes

When `EMTERM_CLI_ONLY` is set:
- Use `cargo build --release --no-default-features` instead of `bun tauri build --no-bundle`
- Skip .desktop file creation
- Skip icon copying
- Skip postinst/postrm GTK cache updates
- Adjust DEBIAN/control: `Section: utils`, `Depends: libc6`, CLI-focused description

## Test Scenarios

### Unit Tests
- [ ] Existing Rust unit tests pass with `--no-default-features`
- [ ] Existing Rust unit tests pass with default features (regression)

### Integration Tests
- [ ] `emterm --help` works in CLI-only binary
- [ ] `emterm --version` works in CLI-only binary
- [ ] `emterm markdown <file>` works in CLI-only binary
- [ ] `emterm image <file>` works in CLI-only binary

### Build Tests
- [ ] `cargo build --no-default-features` succeeds
- [ ] `cargo build` (default features) succeeds
- [ ] `cargo test --no-default-features` succeeds
- [ ] `cargo test` (default features) succeeds

### E2E Tests
**Existing E2E tests**: `e2e-tests/`
**Run command**: `./scripts/run-e2e-docker.sh`
- [ ] Existing E2E tests pass without regression (GUI build only)

### Edge Cases
- [ ] CLI-only binary run without subcommand shows help text (not crash/panic)
- [ ] `#[cfg_attr(not(debug_assertions), windows_subsystem = "windows")]` in main.rs: only apply when `gui` feature is enabled

## Error Handling

### CLI-Only Binary Without Subcommand (FR7)

When a user runs the CLI-only binary without a subcommand (`emterm` with no arguments):
- Display help text via `clap`
- Exit with code 0

## Success Criteria

- [ ] `cargo build --no-default-features` compiles without GUI library dependencies
- [ ] `cargo test --no-default-features` passes all applicable tests
- [ ] `cargo build` (default) produces identical binary to current build
- [ ] `cargo test` (default) passes all existing tests
- [ ] `EMTERM_CLI_ONLY=1 make dpkg` produces a working CLI-only dpkg
- [ ] The CLI-only binary runs `emterm image` and `emterm markdown` correctly

## Open Questions

> **Note**: No unresolved requirements.

## References

- Cargo Features: https://doc.rust-lang.org/cargo/reference/features.html
- Conditional Compilation: https://doc.rust-lang.org/reference/conditional-compilation.html
