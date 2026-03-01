# Implementation Plan: CLI-Only Build

## Overview

Enable building emterm CLI commands (`emterm image`, `emterm markdown`) without GUI library dependencies by introducing a Cargo `gui` feature flag and conditional compilation gates. The build script detects `EMTERM_CLI_ONLY` environment variable to produce a lightweight CLI-only dpkg package.

## Objectives

- Build CLI commands on headless servers without `libwebkit2gtk`, `libgtk-3-0`, etc.
- Provide `EMTERM_CLI_ONLY=1 make dpkg` workflow for CLI-only package generation
- Maintain full backward compatibility for default GUI builds

## Prerequisites

### Development Environment

- Rust 1.85+ (edition 2024)
- Cargo with feature flag support
- Bun (for GUI builds only)

### Dependencies

- Existing emterm codebase with working GUI build
- dpkg-deb for package generation

## Architecture Overview

### Technology Stack

- **Language**: Rust
- **Framework**: Tauri (GUI-only, gated behind feature flag)
- **Build System**: Cargo features + shell script

### Design Approach

Bottom-up approach: modify Cargo.toml dependency declarations first, then add conditional compilation gates at module boundaries, and finally update the build/packaging script. Each phase is independently verifiable.

### Component Interaction

```
Environment Variable        Cargo Feature              Build Result
────────────────────       ─────────────────          ─────────────
EMTERM_CLI_ONLY unset  →   default = ["gui"]      →   Full Tauri binary
EMTERM_CLI_ONLY=1      →   --no-default-features  →   CLI-only binary
```

CLI-compatible modules (`commands::image`, `commands::markdown`, `commands::tmux`, `encoding`, `error`, `protocols`, `validation`) remain unconditionally compiled. GUI modules (`ansi`, `image`, `logging`, `pty`, all Tauri commands/state) are gated behind the `gui` feature.

## Implementation Phases

### Phase 1: Cargo Feature Flag Configuration

**Goal**: Define `gui` feature flag and make all GUI-specific dependencies optional. After this phase, `cargo check --no-default-features` should resolve dependencies without GUI crates.

**Files to Modify**:
- `src-tauri/Cargo.toml` - Add features section, convert GUI dependencies to optional

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| `[features]` section | Define `gui` feature aggregating all GUI deps | No features section exists | `default = ["gui"]` with all GUI crate names listed |
| GUI dependencies | Mark as optional | Currently unconditional | Each GUI dep has `optional = true` |
| `tauri-build` | Make optional build-dep | Currently unconditional | Optional, included in `gui` feature |

**Implementation Steps**:

1. **Add features section** - Define `default = ["gui"]` and `gui` feature listing all GUI-only crate names
2. **Convert GUI dependencies to optional** - Add `optional = true` to: tauri, tauri-plugin-clipboard-manager, tauri-plugin-notification, tauri-plugin-shell, portable-pty, tokio, futures, font-kit
3. **Convert build dependency** - Make tauri-build optional and include in gui feature
4. **Verify dependency resolution** - Ensure `cargo check --no-default-features` resolves without fetching GUI crates

**Dependencies**: None (first phase)

**Testing Approach**:
- Build: `cargo check --no-default-features` resolves deps (will not compile yet due to code references)
- Build: `cargo check` (default features) still works

**Acceptance Criteria**:
- [ ] `cargo metadata --no-default-features` shows no GUI crate dependencies
- [ ] `cargo check` with default features still resolves correctly

**Estimated Effort**: Small

---

### Phase 2: Conditional Compilation Gates

**Goal**: Add `#[cfg(feature = "gui")]` gates at module boundaries so that `cargo build --no-default-features` compiles successfully, producing a binary with CLI commands only.

**Files to Modify**:
- `src-tauri/build.rs` - Gate Tauri build step
- `src-tauri/src/lib.rs` - Gate GUI module declarations and all Tauri-dependent code
- `src-tauri/src/commands/mod.rs` - Gate GUI-only submodules (config, font, editor)
- `src-tauri/src/main.rs` - Gate `app_lib::run()` call and `windows_subsystem` attribute

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| build.rs gate | Skip Tauri build when gui feature disabled | Unconditionally calls tauri_build | Only calls tauri_build with gui feature |
| lib.rs module gates | Conditionally declare GUI modules | All modules unconditional | `ansi`, `image`, `logging`, `pty` gated; `commands`, `encoding`, `error`, `protocols`, `validation` always available |
| lib.rs Tauri code gate | Conditionally compile all Tauri imports, state types, command functions, and run() | All Tauri code unconditional | All Tauri-dependent code gated behind gui feature |
| commands/mod.rs gates | Conditionally declare GUI submodules | All submodules unconditional | `config`, `font`, `editor` gated; `image`, `markdown`, `tmux` always available |
| main.rs entry point | Route to help or GUI based on feature | Falls through to app_lib::run() | gui: run(), no-gui: show help and exit |
| main.rs windows_subsystem | Only apply when GUI enabled | Unconditional attribute | Gated behind gui feature |

**Processing Flow**:
1. build.rs executes
   - gui feature enabled → call Tauri build + git version
   - gui feature disabled → git version only
2. lib.rs compiles
   - gui feature enabled → all modules + Tauri commands
   - gui feature disabled → CLI-compatible modules only
3. main.rs executes
   - Subcommand provided → route to CLI handler (both builds)
   - No subcommand + gui → launch Tauri app
   - No subcommand + no gui → display help, exit 0

**Implementation Steps**:

1. **Gate build.rs** - Wrap Tauri build call with conditional compilation
2. **Gate lib.rs modules** - Add feature gates to GUI-only module declarations (ansi, image, logging, pty)
3. **Gate lib.rs Tauri code** - Wrap all Tauri imports, state structs, command functions, and run() function with feature gate
4. **Gate commands/mod.rs** - Add feature gates to config, font, editor submodule declarations
5. **Gate main.rs entry** - Add dual-path logic for no-subcommand case and conditional windows_subsystem attribute

**Dependencies**: Requires Phase 1

**Testing Approach**:
- Build: `cargo build --no-default-features` succeeds
- Build: `cargo build` (default) succeeds
- Unit: `cargo test --no-default-features` passes
- Unit: `cargo test` (default) passes
- Integration: CLI-only binary responds to `--help`, `--version`

**Acceptance Criteria**:
- [ ] `cargo build --no-default-features` compiles without errors
- [ ] `cargo build` produces identical binary to current build
- [ ] `cargo test --no-default-features` passes all applicable tests
- [ ] `cargo test` passes all existing tests
- [ ] CLI-only binary shows help when run without subcommand

**Estimated Effort**: Medium

---

### Phase 3: Build Script CLI-Only Support

**Goal**: Modify `build-dpkg.sh` to detect `EMTERM_CLI_ONLY` environment variable and produce a CLI-only dpkg package without GUI dependencies or desktop integration files.

**Files to Modify**:
- `scripts/build-dpkg.sh` - Add CLI-only build and packaging path

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| Environment detection | Check EMTERM_CLI_ONLY var | Not checked | Early detection, sets build mode |
| Build command selection | Choose cargo vs bun tauri build | Always bun tauri build | CLI-only: cargo build --release --no-default-features |
| Binary path resolution | Locate built binary | Tauri output path | CLI-only: cargo target/release path |
| Package content | Control what goes into dpkg | Desktop file, icons, binary | CLI-only: binary only + docs |
| Control file | Define package metadata | GUI dependencies, Section: x11 | CLI-only: Section: utils, Depends: libc6, CLI description |
| Maintainer scripts | postinst/postrm for GTK cache | Always included | CLI-only: skip GTK cache update scripts |

**Processing Flow**:
1. Script starts
   - EMTERM_CLI_ONLY set → enter CLI-only build path
   - EMTERM_CLI_ONLY unset → existing GUI build path (unchanged)
2. CLI-only build path:
   - Run cargo build with --release --no-default-features
   - Create dpkg structure with binary only (no desktop/icons)
   - Generate control file with CLI-specific metadata
   - Skip postinst/postrm GTK cache updates
   - Build dpkg with dpkg-deb
3. GUI build path: unchanged from current behavior

**Implementation Steps**:

1. **Add environment detection** - Check `EMTERM_CLI_ONLY` variable early in script
2. **Add CLI-only build command** - Use `cargo build --release --no-default-features` instead of `bun tauri build`
3. **Adjust binary path** - Resolve binary from `target/release/` instead of Tauri output directory
4. **Conditionally skip GUI assets** - Skip desktop file creation, icon copying when CLI-only
5. **Generate CLI-specific control file** - Section: utils, minimal dependencies, CLI-focused description
6. **Skip GUI maintainer scripts** - Omit postinst/postrm GTK cache update when CLI-only

**Dependencies**: Requires Phase 1 and Phase 2

**Testing Approach**:
- Build: `EMTERM_CLI_ONLY=1 make dpkg` succeeds
- Build: `make dpkg` (default) still produces GUI package
- Integration: CLI-only dpkg installs and `emterm image`/`emterm markdown` work
- Integration: GUI dpkg unchanged from current behavior

**Acceptance Criteria**:
- [ ] `EMTERM_CLI_ONLY=1 make dpkg` produces a working dpkg
- [ ] CLI-only dpkg has no GUI-related dependencies in control file
- [ ] CLI-only dpkg contains no desktop file or icons
- [ ] `make dpkg` (default) produces identical output to current behavior
- [ ] CLI-only binary from dpkg executes `emterm image` and `emterm markdown` correctly

**Estimated Effort**: Medium

---

## Complete File Structure

```
src-tauri/
├── Cargo.toml           # Add [features] section, make GUI deps optional
├── build.rs             # Gate tauri_build::build() with cfg(feature = "gui")
└── src/
    ├── main.rs          # Gate app_lib::run() and windows_subsystem with cfg
    ├── lib.rs           # Gate GUI modules and Tauri code with cfg(feature = "gui")
    └── commands/
        └── mod.rs       # Gate config, font, editor with cfg(feature = "gui")

scripts/
└── build-dpkg.sh        # Detect EMTERM_CLI_ONLY, adjust build + packaging
```

## Testing Strategy

- **Unit**: Existing Rust unit tests pass with both `--no-default-features` and default features
- **Integration**: CLI-only binary responds to `--help`, `--version`, `emterm image`, `emterm markdown`
- **E2E (Docker)**: Existing E2E tests pass without regression (GUI build)
- **Manual**: `EMTERM_CLI_ONLY=1 make dpkg` on headless server, install and verify CLI commands

## Dependencies

| Package | Version | Purpose |
|---------|---------|---------|
| clap | 4.5.54 | CLI argument parsing (always-on) |
| tauri | 2.9.5 | GUI framework (optional, gui feature) |
| tauri-build | 2.5.3 | Build-time Tauri integration (optional, gui feature) |

## Risk Assessment

| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|------------|
| Hidden GUI dependency in CLI modules | Low | Medium | Verify `cargo build --no-default-features` compiles cleanly |
| Existing tests broken by optional deps | Low | High | Run full test suite with both feature configurations |
| Build script path differences across platforms | Medium | Medium | Test on both Linux architectures (x86_64, aarch64) |

## Open Questions

None. All requirements are fully specified.

## Success Metrics

- [ ] `cargo build --no-default-features` compiles without GUI library dependencies
- [ ] `cargo test --no-default-features` passes all applicable tests
- [ ] `cargo build` (default) produces identical binary to current build
- [ ] `cargo test` (default) passes all existing tests
- [ ] `EMTERM_CLI_ONLY=1 make dpkg` produces a working CLI-only dpkg
- [ ] CLI-only binary executes `emterm image` and `emterm markdown` correctly
