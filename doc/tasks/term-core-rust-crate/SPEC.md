# Feature: term_core Rust Crate Extraction (Phase 2)

## Overview

Restructure the existing `wasm/src/` (a ~15k LOC ANSI parser / terminal grid / Unicode processing library currently built as a wasm-bindgen cdylib) into a pure Rust crate at `crates/term_core/`. The existing `wasm/` directory is rewritten as a thin wrapper that imports `term_core` and re-exposes it through wasm-bindgen so the current Tauri build keeps working. Establishes a root Cargo workspace and prepares for Phase 3, where the native terminal core consumes `term_core` directly via a `path` dependency.

## Objectives

- Make the terminal core library callable from native Rust without going through a WASM bridge.
- Keep the current Tauri build (`bun tauri build`) working untouched throughout Phase 2.
- Establish a Cargo workspace at the repository root.
- Migrate the existing wasm/src test suite to `cargo test` and keep it green.
- Replace Phase 1 PoC's stand-in parser/grid (`native-poc/src/{parser,grid}/`) with `term_core` once the crate is ready.

## User Stories

### US1: Build the workspace from the repository root

As an eMterm developer, I want to run `cargo build --workspace` and `cargo test --workspace` once, so that all Rust crates in the repository (`src-tauri`, `wasm`, `native-poc`, `crates/term_core`) build and test in a single invocation.

**Acceptance Criteria:**
- [ ] A root `Cargo.toml` exists with a `[workspace]` section listing all four members.
- [ ] `cargo build --workspace` succeeds locally.
- [ ] `cargo test --workspace` succeeds locally.

### US2: Use term_core from native Rust

As an eMterm developer (Phase 3), I want to add `term_core = { path = "../crates/term_core" }` to a Cargo.toml and call its APIs directly, so that Phase 3 work proceeds without a WASM bridge.

**Acceptance Criteria:**
- [ ] `crates/term_core/Cargo.toml` declares no dependency on `wasm-bindgen`, `js-sys`, `web-sys`, or `serde-wasm-bindgen`.
- [ ] A consumer crate can compile against `term_core` on the host (`x86_64-unknown-linux-gnu`).
- [ ] The public API surface is sufficient to drive an ANSI byte stream into the grid and observe cursor/cell/scrollback state.

### US3: Keep the Tauri build green

As an eMterm developer / end user, I want `bun tauri build` and `bun test` to keep working after Phase 2, so that the existing application is unaffected.

**Acceptance Criteria:**
- [ ] `wasm-pack build wasm/ --target web` (or the existing `wasm:build` script) produces `wasm/pkg/` with a Tauri-compatible shape.
- [ ] `bun tauri build` succeeds.
- [ ] `bun test` (TypeScript) passes.

### US4: Port existing tests to cargo test

As an eMterm developer, I want the original wasm/src unit tests (notably `parser/tests.rs`, ~1k LOC) to run under `cargo test`, so that the migration cannot silently drop coverage.

**Acceptance Criteria:**
- [ ] All existing `mod tests` blocks under `crates/term_core/src/` run as `cargo test`.
- [ ] No tests are silently dropped — anything skipped is recorded in the verification result with a reason.

### US5: Retire Phase 1 PoC stand-ins

As an eMterm developer, I want `native-poc/src/parser/` and `native-poc/src/grid/` to be removed, so that there is exactly one ANSI core implementation across the codebase.

**Acceptance Criteria:**
- [ ] `native-poc/src/parser/` and `native-poc/src/grid/` no longer exist.
- [ ] `native-poc/Cargo.toml` depends on `term_core` via `path`.
- [ ] `cargo build --manifest-path native-poc/Cargo.toml` succeeds.
- [ ] Pre-existing native-poc tests (selection, pty input, etc.) still pass.

## Technical Requirements

### Functional Requirements

- **FR1 — Cargo workspace at repository root.** A `Cargo.toml` is created at the repo root containing `[workspace]` with `members = ["src-tauri", "wasm", "native-poc", "crates/term_core"]` and `resolver = "2"`.
- **FR2 — Code relocation via `git mv`.** Every file under `wasm/src/` is moved to `crates/term_core/src/` with `git mv` to preserve history.
- **FR3 — wasm-bindgen strip in term_core.** `crates/term_core/` contains no `#[wasm_bindgen]` annotations and no dependency on `wasm-bindgen`, `js-sys`, `web-sys`, or `serde-wasm-bindgen`. `JsValue`, `Uint8Array`, and similar bridge types are replaced by their Rust-native equivalents (`&[u8]`, `Vec<u8>`, slices, owned structs, etc.).
- **FR4 — Callbacks become a Rust trait.** The previous wasm-bindgen `js_sys::Function` callbacks for terminal events (bell, title change, resize request, visual bell, viewer/OSC dispatch, etc.) are abstracted into a `TerminalCallbacks` trait inside `term_core`. The trait covers exactly the callbacks the existing wasm crate exposed today; no new callbacks are added.
- **FR5 — `wasm/` becomes a thin wrapper.** `wasm/Cargo.toml` keeps `crate-type = ["cdylib"]` and `name = "emterm-wasm"`. It adds `term_core = { path = "../crates/term_core" }` and the previously-existing wasm-bindgen-side dependencies. `wasm/src/lib.rs` is the only required source file; it forwards all logic to `term_core` and provides a wasm-bindgen-flavored implementation of `TerminalCallbacks` over `js_sys::Function`.
- **FR6 — Test suite ports to cargo test.** All `#[test]` and `mod tests` under the moved sources continue to pass under `cargo test -p term_core` (and `cargo test --workspace`). Tests that referenced wasm-only types are rewritten to call the same logic through the new Rust API or removed only if their coverage is duplicated.
- **FR7 — TS-facing exports are functionally identical.** The wasm-bindgen-exported function names, parameter shapes, and return value shapes seen from TypeScript stay equivalent across the migration. The TypeScript side does not need to change its import paths or call sites.
- **FR8 — native-poc switches to term_core.** `native-poc/src/parser/` and `native-poc/src/grid/` are deleted. `native-poc` builds against `term_core` via a workspace path dependency. `native-poc/src/tabs.rs`, `render/mod.rs`, and `window_host.rs` are adjusted to use `term_core` types.

### Non-Functional Requirements

- **NFR1 — Tauri build stays green.** `bun tauri build` succeeds on the same machine where Phase 2 is implemented, both during work-in-progress (after FR5 lands) and at Phase 2 completion.
- **NFR2 — Build time parity.** `cargo build -p term_core` (native target) is not slower than the current `cargo build` for the wasm32 target of `wasm/`. No numeric threshold; informally sampled.
- **NFR3 — Module layout preservation.** `crates/term_core/src/` keeps the existing module layout of `wasm/src/` (parser/, csi_*, terminal_*, sgr, slim_cell, ring_buffer, reflow, snapshot, char_table, style_table, color_spec, esc/osc/apc/c0/print handlers, lib.rs). Phase 2 is not a refactor of internal module boundaries.
- **NFR4 — No silent test loss.** Any test removed during the migration is recorded in VERIFICATION_RESULT.md with a reason.
- **NFR5 — Linux-only validation.** Verification runs on Linux (Ubuntu 22.04 family) only. Windows-specific re-validation is out of scope and inherits whatever the existing build supports.

## Implementation Approach

### Architecture

**Before Phase 2:**

```
emterm/
├── src-tauri/          (Tauri Rust backend, calls wasm via TS bridge only)
├── src/                (TypeScript, imports wasm/pkg)
├── wasm/               (cdylib, wasm-bindgen + ANSI core logic, ~15k LOC)
│   ├── Cargo.toml
│   └── src/
└── native-poc/         (Phase 1 PoC, ships its own stand-in parser/grid)
```

**After Phase 2:**

```
emterm/
├── Cargo.toml                  ← NEW: workspace root
├── src-tauri/                  (workspace member, unchanged)
├── src/                        (TypeScript, imports unchanged)
├── crates/
│   └── term_core/              ← NEW: pure Rust crate (lifted from wasm/src/)
│       ├── Cargo.toml
│       └── src/                (the ANSI core, no wasm-bindgen)
├── wasm/                       (workspace member, now a thin wrapper)
│   ├── Cargo.toml              (cdylib, depends on term_core)
│   └── src/lib.rs              (wasm-bindgen re-exports, ~hundreds of LOC)
└── native-poc/                 (workspace member, depends on term_core)
    └── src/                    (parser/ and grid/ removed; tabs/render rewired)
```

### Component Interaction

```
TypeScript (src/)
    ↓ import wasm/pkg
wasm (thin wrapper, cdylib)
    ↓ pure Rust call
crates/term_core (pure Rust)
    ↑ pure Rust call
native-poc (binary, Phase 3+ ターミナル本体)
    ↑ pure Rust call
src-tauri (Tauri host process; today calls wasm via TS only)
```

### Data Flow

PTY bytes flow into `term_core::Parser`, which mutates `term_core::Grid` (cells, cursor, scrollback, alt-screen state) and emits callbacks via the `TerminalCallbacks` trait. Existing TS callers see this through wasm-bindgen exports unchanged; future native callers (Phase 3) implement the trait directly.

### API Design

term_core's public surface is the moved-and-cleaned API of `wasm/src/`. The migration must NOT redesign the API; it must:

1. Remove `#[wasm_bindgen]` attributes from public items.
2. Replace `JsValue` / `Uint8Array` parameter and return types with native Rust types of equivalent meaning (`&[u8]`, `Vec<u8>`, owned structs, `&str`).
3. Replace `js_sys::Function` callbacks with calls through `TerminalCallbacks`.
4. Move `serde-wasm-bindgen`-mediated structs to plain `serde::{Serialize, Deserialize}` (the thin wrapper handles the JsValue conversion).

#### `TerminalCallbacks` trait sketch

The exact set is derived during implementation by grepping for `js_sys::Function` use sites and the existing `callbacks.rs`. Today's known callback categories include:

- Title change (OSC 0/2).
- Bell (BEL).
- Visual bell.
- Cursor visibility change.
- Resize request from the application.
- emterm OSC extension dispatch (Markdown / image viewer launch).
- Any other callback discovered in the current `callbacks.rs`.

Each becomes a trait method with signatures expressed in native Rust types. The wasm-bindgen side implements this trait with a struct that owns the corresponding `js_sys::Function` handles.

### Dependencies

**Workspace root `Cargo.toml`:**

- `[workspace]` with `members = ["src-tauri", "wasm", "native-poc", "crates/term_core"]`.
- `resolver = "2"`.
- `[workspace.package]` optional — left to implementer (not required by FR).

**`crates/term_core/Cargo.toml`:**

- Pure Rust dependencies only.
- Likely keepers: `serde`, `unicode-width`, `log`, possibly `bincode` (verified during implementation).
- **Forbidden**: `wasm-bindgen`, `js-sys`, `web-sys`, `serde-wasm-bindgen`, `console_error_panic_hook`, any `wasm32-*`-only crate.

**`wasm/Cargo.toml` (thin wrapper):**

- `[lib].crate-type = ["cdylib"]`.
- `[lib].name` (or `[package].name`) `emterm-wasm` (preserves TS-side package name).
- Dependencies: `term_core = { path = "../crates/term_core" }`, `wasm-bindgen`, `js-sys`, `web-sys`, `serde-wasm-bindgen`, `serde`, `bincode` (if needed).
- `[package.metadata.wasm-pack.profile.release]` `wasm-opt = false` preserved.

**`native-poc/Cargo.toml`:**

- Adds `term_core = { path = "../crates/term_core" }`.
- Removes any `parser` / `grid` references from its own source modules.

**`src-tauri/Cargo.toml`:**

- No changes (it never depended on the wasm crate directly; it crossed through the TS layer).

### File Structure (Phase 2 deliverables)

```
Cargo.toml                              # NEW: workspace root
crates/term_core/Cargo.toml             # NEW
crates/term_core/src/                   # MOVED from wasm/src/ via git mv
    parser/                             #   (existing module tree preserved)
    parser/tests.rs                     #   stays in place; runs as cargo test
    csi_*.rs
    terminal_*.rs
    sgr.rs, slim_cell.rs, ring_buffer.rs
    reflow.rs, snapshot.rs, char_table.rs
    style_table.rs, color_spec.rs
    osc_handler.rs, apc_handler.rs, esc_handler.rs, c0_handler.rs, print_handler.rs
    callbacks.rs                        #   replaced by TerminalCallbacks trait
    lib.rs                              #   no #[wasm_bindgen]; pure Rust pub API
wasm/Cargo.toml                         # REWRITTEN to depend on term_core
wasm/src/lib.rs                         # NEW thin wrapper (only file in wasm/src/)
native-poc/Cargo.toml                   # adds term_core dependency
native-poc/src/                         # parser/ and grid/ removed
native-poc/src/tabs.rs                  # rewired to term_core
native-poc/src/render/mod.rs            # rewired to term_core
native-poc/src/window_host.rs           # rewired (Grid handle source changes)
```

## Test Scenarios

### Unit Tests

| ID | Scenario | Expected | Type |
|----|----------|----------|------|
| TS-1 | All `mod tests` in moved sources pass under `cargo test -p term_core` | All pass | Unit |
| TS-2 | `parser/tests.rs` passes (~1k LOC) | All pass | Unit |
| TS-3 | term_core compiles without any wasm-bindgen-family dependency in scope | `cargo metadata` shows no forbidden crates among term_core deps | Static |

### Integration Tests

| ID | Scenario | Expected | Type |
|----|----------|----------|------|
| TS-4 | `cargo build --workspace` succeeds | exit 0 | Integration |
| TS-5 | `cargo test --workspace` succeeds | exit 0 | Integration |
| TS-6 | `wasm-pack build wasm/ --target web` produces `wasm/pkg/` | pkg/ exists with TS bindings | Integration |
| TS-7 | `bun tauri build` succeeds with the new thin wrapper in place | exit 0 | Integration |
| TS-8 | `bun test` (TS) passes | All pass | Integration |
| TS-9 | native-poc builds and its existing tests pass after term_core swap | `cargo build` + `cargo test` exit 0 | Integration |

### Manual / Verification

| ID | Scenario | Expected | Type |
|----|----------|----------|------|
| TS-10 | `bun tauri dev` opens a window and ANSI rendering works as before the refactor | functionally equivalent | Manual |
| TS-11 | Build time of `cargo build -p term_core` is not noticeably slower than the previous `cargo build` for wasm/ | informal feel | Manual |

### Edge Cases

- [ ] A test in `wasm/src/` that referenced `JsValue` directly — rewrite to call the Rust API or replace with an equivalent pure-Rust assertion.
- [ ] `console_error_panic_hook` calls — move to the thin wrapper, drop from term_core.
- [ ] `bincode` encode/decode that crossed the wasm boundary — keep only if a native consumer needs it; otherwise move into the thin wrapper.

### Performance / Security

- No new performance targets. NFR2 is informal.
- No security surface change.

## Security Considerations

- **Surface change**: none. term_core is an internal library and the wasm boundary surface is unchanged from the TS perspective.
- **Supply chain**: removing four wasm-only crates from `term_core/Cargo.toml` reduces the native build's transitive dependency set. The thin wrapper retains them.

## Error Handling

term_core's error model is whatever wasm/src/ shipped today. Error types that were exposed via `Result<JsValue, JsValue>` become `Result<T, term_core::Error>` (concrete type defined per the existing usage). The thin wrapper converts these to JsValue for the TS side.

## Performance Optimization

No optimization work in Phase 2. The migration is mechanical.

## Success Criteria

- [ ] FR1–FR8 are demonstrably satisfied.
- [ ] All Test Scenarios pass.
- [ ] `bun tauri build` and `bun test` are green.
- [ ] `crates/term_core/Cargo.toml` carries zero wasm-bindgen-family dependencies.
- [ ] `native-poc/src/parser/` and `native-poc/src/grid/` have been removed.

## Open Questions

> Tracked in `sdd.yaml` as `status: assumed` where decisions are made; resolved during create-plan / implement.

- [ ] Rust edition of `term_core` (assume: keep the source's existing 2024 edition; if workspace requires uniformity, downgrade per implementer judgment).
- [ ] Final shape of `TerminalCallbacks` trait (assumed to mirror the existing `callbacks.rs` 1:1).
- [ ] Whether `bincode` is needed by `term_core` proper (assumed: only if used by a native consumer; the thin wrapper otherwise carries it).
- [ ] Whether `wasm-pack` output path moves (assumed: no; keep `wasm/pkg/`).

## Implementation Phases

This SPEC corresponds to restruct.md's Phase 2. See restruct.md "フェーズ全体像" for the surrounding phase chain (Phase 1 PoC → Phase 2 term_core → Phase 3 native terminal features).

## References

- `tmp/restruct.md` — Restructuring strategy (Phase 2 detail section).
- `wasm/Cargo.toml`, `wasm/src/` — Source of the migration.
- `doc/tasks/native-terminal-poc/` — Phase 1 SDD outputs.
- `native-poc/` — Phase 1 PoC binary, target of FR8.
