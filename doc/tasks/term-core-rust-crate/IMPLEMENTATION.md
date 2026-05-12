# Implementation Plan: term_core Rust Crate Extraction (Phase 2)

## Overview
Lift `wasm/src/` into a new pure Rust crate `crates/term_core/`, strip wasm-bindgen from the core, rewrite `wasm/` as a thin wrapper that imports `term_core`, and migrate `native-poc` off its Phase 1 stand-in modules.

## Objectives
- Establish a workspace root and keep the existing builds green at every checkpoint.
- Produce a pure Rust `term_core` crate with no wasm-bindgen surface.
- Keep `bun tauri build` / `bun test` working at the end of Phase 2.
- Have `native-poc` depend on `term_core` for parser and grid.

## Prerequisites

### Development Environment
- Linux (Ubuntu 22.04 family) — same host as Phase 1.
- Rust stable toolchain matching existing crates.
- `wasm-pack` installed (already required by current Tauri build).
- Bun installed (already required).

### Dependencies
- Phase 1 PoC must remain compileable; its sources are still under `native-poc/` but `native-poc/src/parser/` and `native-poc/src/grid/` will be removed at the end of Phase 2.
- The existing Tauri build (`bun tauri build`) must work on the same branch (`refactor/native-terminal-hybrid`) before and after Phase 2.

## Architecture Overview

### Technology Stack
- **Language**: Rust (term_core, thin wrapper, native-poc, src-tauri); TypeScript build pipeline kept unchanged.
- **Workspace**: Cargo workspace at the repository root, `resolver = "2"`.
- **WASM toolchain**: `wasm-pack` continues to drive `wasm/pkg/` generation.

### Design Approach
- A migration, not a redesign. The module tree under `wasm/src/` is preserved verbatim in `crates/term_core/src/`. Only the wasm-bindgen surface is removed from term_core and re-implemented in the thin wrapper.
- Callbacks formerly delivered by `js_sys::Function` become a single `TerminalCallbacks` trait in term_core. The thin wrapper provides one implementation that holds the JS functions; native consumers provide their own.
- Workspace is established **first** so subsequent moves do not break the Tauri build mid-flight.
- A short-lived shim window exists in Phase 4 where the thin wrapper exists and `wasm/pkg/` builds, but TS-side validation has not yet run.

### Component Interaction

```
TypeScript (src/)  ────import────►  wasm/pkg/ (cdylib build of wasm/)
                                          │
                                          │ Rust call (path dep)
                                          ▼
                                  crates/term_core (pure Rust)
                                          ▲
                                          │ Rust call (path dep)
                                          │
                                 native-poc (binary, Phase 3+)
```

## Implementation Phases

### Phase 1: Workspace bootstrap

**Goal**: A root `Cargo.toml` workspace exists with the four members; `cargo build --workspace` and `bun tauri build` both still succeed.

**Files to Create**:
- `Cargo.toml` — workspace root.

**Files to Modify**:
- `.gitignore` — already excludes `target/`; no change unless workspace introduces a new path.

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| Workspace Cargo.toml | Lists members (src-tauri, wasm, native-poc) and sets resolver = "2" | Cargo present | `cargo build --workspace` succeeds |

**Processing Flow** (diagram-convertible):
1. Create `Cargo.toml` at repo root.
2. List existing members: `["src-tauri", "wasm", "native-poc"]` (term_core added in Phase 2 of this plan).
3. Run `cargo build --workspace`.
   - Success → proceed.
   - Failure → diagnose and resolve (most likely culprit: edition or feature flag mismatch).
4. Run `bun tauri build` smoke check.

**Implementation Steps**:
1. **Create root Cargo.toml** with `[workspace]`, `members`, `resolver`.
2. **Verify `cargo build --workspace`** succeeds.
3. **Verify `bun tauri build`** still succeeds (regression check; no logical change yet).
4. **Capture pre-migration baseline** (used by Phase 4 TS-12 and Phase 4 inventory step):
   - Run `bun run wasm:build` (or the existing script) to ensure `wasm/pkg/` is fresh.
   - Copy `wasm/pkg/*.d.ts`, `wasm/pkg/*.js`, and `wasm/pkg/package.json` to `tmp/term-core-baseline/pkg/`.
   - Grep `wasm/src/` for `#[wasm_bindgen]` declarations (functions, methods, structs) and save the list as `tmp/term-core-baseline/wasm-bindgen-exports.txt`.
   - Grep `wasm/src/` for `js_sys::Function` and `extern "C"` callback sites; save as `tmp/term-core-baseline/js-callback-sites.txt` for Phase 2 trait design.

**Dependencies**: None.

**Testing Approach**:
- Integration: workspace build succeeds.
- Manual: Tauri build succeeds.

**Acceptance Criteria**:
- [ ] `Cargo.toml` exists at repo root with the three existing members.
- [ ] `cargo build --workspace` exits 0.
- [ ] `bun tauri build` exits 0 unchanged.
- [ ] `tmp/term-core-baseline/pkg/` contains the pre-migration `.d.ts` / `.js` / `package.json` for Phase 4 export-shape diff.
- [ ] `tmp/term-core-baseline/wasm-bindgen-exports.txt` and `tmp/term-core-baseline/js-callback-sites.txt` exist and are non-empty.

**Estimated Effort**: small.

---

### Phase 2: term_core extraction (code move + wasm-bindgen strip)

**Goal**: `crates/term_core/` exists as a pure Rust crate. All wasm-bindgen-family dependencies are removed from term_core. `cargo build -p term_core` succeeds.

**Files to Create**:
- `crates/term_core/Cargo.toml` — pure Rust crate manifest.
- `crates/term_core/src/callbacks.rs` — replaced by `TerminalCallbacks` trait (file kept at same path; contents rewritten).

**Files to Move (via `git mv`)**:
- Every Rust source file under `wasm/src/` → `crates/term_core/src/`.

**Files to Modify**:
- `Cargo.toml` (root) — add `crates/term_core` to `members`.

**Files to Delete (after move)**:
- `wasm/src/*.rs` other than the moved set are not expected; if any leftover exists, audit.

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| `crates/term_core/Cargo.toml` | Pure Rust crate manifest with `serde`, `unicode-width`, `log` (+ `bincode` if used by core) | git mv complete | term_core builds against `x86_64-unknown-linux-gnu` |
| `TerminalCallbacks` trait | Replaces `js_sys::Function` callbacks | Identified callback sites | Native and wasm consumers can implement the trait |
| term_core internal modules | Same as wasm/src/ module tree | Code moved | Public API surface is functionally identical minus JsValue/Uint8Array |

**Processing Flow** (diagram-convertible):
1. Create `crates/term_core/` directory with a fresh `Cargo.toml`.
2. `git mv wasm/src/*.rs crates/term_core/src/` and `git mv wasm/src/parser/ crates/term_core/src/parser/`.
3. **Immediately after the move, install a workspace-safe stub at `wasm/src/lib.rs`**: a minimal empty crate body (`//! placeholder during Phase 2 migration; rebuilt in Phase 4`). Without this step, the `wasm` workspace member fails to compile while term_core is being cleaned up, and `cargo build --workspace` stays broken longer than necessary.
4. Add `crates/term_core` to workspace members; `cargo build --workspace` should now compile term_core (with errors during the wasm-bindgen strip) and trivially compile the empty wasm stub.
5. Resolve term_core compile errors in three passes:
   - Pass A: remove `#[wasm_bindgen]` attributes and `extern "C"` blocks.
   - Pass B: replace `JsValue`, `Uint8Array`, `js_sys::Function`, and `serde-wasm-bindgen` usage with Rust-native equivalents. For `js_sys::Function` consumers, introduce calls to a `TerminalCallbacks` trait method.
   - Pass C: rewrite `callbacks.rs` to define `pub trait TerminalCallbacks` with one method per existing callback site identified by the Phase 1 grep (`tmp/term-core-baseline/js-callback-sites.txt`).
6. `cargo build -p term_core` until clean. The wasm crate stays as a stub until Phase 4 rebuilds it.

**Implementation Steps**:
1. **Bootstrap `crates/term_core/Cargo.toml`** with pure Rust dependencies and the same `edition` as the source.
2. **`git mv` the source tree** preserving the existing module layout.
3. **Install wasm stub** at `wasm/src/lib.rs` (empty module body) so the workspace keeps compiling during the strip.
4. **Add term_core to workspace** and run `cargo build --workspace` to see the failure surface — only term_core should fail; the wasm stub compiles trivially.
5. **Define `TerminalCallbacks`** based on the Phase 1 baseline list (`tmp/term-core-baseline/js-callback-sites.txt`) plus the contents of the existing `callbacks.rs`.
6. **Mechanical fixups**: remove `#[wasm_bindgen]`, swap `JsValue` for native types, swap `Uint8Array` for `&[u8]`/`Vec<u8>`.
7. **Drop wasm-only crates** (`wasm-bindgen`, `js-sys`, `web-sys`, `serde-wasm-bindgen`, `console_error_panic_hook`) from term_core's manifest. Leave `bincode` only if a native consumer uses it; otherwise move it to the thin wrapper in Phase 4.

**Dependencies**: Phase 1.

**Testing Approach**:
- Integration: `cargo build -p term_core` exits 0.
- Static: `cargo metadata -p term_core` shows no dependency on `wasm-bindgen` / `js-sys` / `web-sys` / `serde-wasm-bindgen`.

**Acceptance Criteria**:
- [ ] `crates/term_core/Cargo.toml` exists and lists only pure-Rust dependencies.
- [ ] `cargo build -p term_core` exits 0.
- [ ] `TerminalCallbacks` is declared and used in place of every former `js_sys::Function` call site.
- [ ] `wasm/src/` no longer contains the moved files (it is temporarily empty or contains only a placeholder; the thin wrapper is created in Phase 4).

**Estimated Effort**: large (the bulk of Phase 2; ~15k LOC touched).

---

### Phase 3: Port the test suite to cargo test

**Goal**: All `#[test]` and `mod tests` from the moved sources run under `cargo test -p term_core` and pass. No test is silently dropped.

**Files to Modify**:
- Any `tests` module inside `crates/term_core/src/**/*.rs` whose body referenced wasm-only types — rewrite to call the pure Rust API.

**Files to Create**:
- `crates/term_core/tests/` — only if an existing integration-style test cannot stay as `mod tests` (unlikely; prefer in-source modules).

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| Migrated unit tests | Verify parser/grid/scroll/SGR behaviour | Phase 2 done | `cargo test -p term_core` passes |
| Migration audit | Track every dropped or rewritten test | Phase 2 done | Each test is either present, rewritten, or recorded in the verification result with a reason |

**Processing Flow**:
1. Run `cargo test -p term_core --no-run`; capture build errors.
2. For each failing test source, classify:
   - Pure Rust assertion → no change needed; the wasm-bindgen strip already fixed it.
   - References `JsValue` / `Uint8Array` directly → rewrite to use the Rust API.
   - Tied to a wasm-specific runtime (highly unlikely for unit tests) → rewrite or drop and log.
3. Iterate until `cargo test -p term_core` passes.

**Implementation Steps**:
1. **Run the suite and triage failures.**
2. **Mechanically rewrite** uses of removed wasm types in tests.
3. **Document drops**: keep a running list to surface in VERIFICATION_RESULT.md (Phase 7 / verify step).

**Dependencies**: Phase 2.

**Testing Approach**:
- Integration: `cargo test -p term_core` passes.
- Audit: no test silently disappears.

**Acceptance Criteria**:
- [ ] `cargo test -p term_core` exits 0 with at least the previous test count.
- [ ] Any dropped tests are listed with reasons in the plan or verification result.

**Estimated Effort**: medium.

---

### Phase 4: Rebuild `wasm/` as a thin wrapper

**Goal**: `wasm/` is a thin wrapper crate that imports `term_core`, implements `TerminalCallbacks` over `js_sys::Function`, and exposes the same wasm-bindgen surface the TS side previously consumed.

**Files to Modify**:
- `wasm/Cargo.toml` — rewrite the manifest: keep `crate-type = ["cdylib"]` and `name = "emterm-wasm"`; depend on `term_core = { path = "../crates/term_core" }`; keep `wasm-bindgen`, `js-sys`, `web-sys`, `serde-wasm-bindgen`, `serde`, `bincode` (if used by the JS bridge).

**Files to Create**:
- `wasm/src/lib.rs` — the wrapper. Imports term_core, defines a `JsCallbacks` struct that owns `js_sys::Function` handles and implements `TerminalCallbacks`, and re-exposes the wasm-bindgen API at parity with the previous build.

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| `wasm/src/lib.rs` | Re-exposes term_core via wasm-bindgen | Phase 3 done | wasm-pack build succeeds |
| `JsCallbacks` | Implements `TerminalCallbacks` calling `js_sys::Function` | TerminalCallbacks finalized | Bridges trait methods to JS callbacks |

**Processing Flow**:
1. Rewrite `wasm/Cargo.toml`.
2. Author `wasm/src/lib.rs` with:
   - `#[wasm_bindgen]` constructor that produces a term_core handle bound to a `JsCallbacks`.
   - `#[wasm_bindgen]` methods that the TS side called previously, each delegating to term_core.
   - `JsCallbacks` struct holding `js_sys::Function` for every `TerminalCallbacks` method, with corresponding trait impl.
3. Run `wasm-pack build wasm/ --target web` (or the existing `wasm:build` script).
4. Inspect `wasm/pkg/` for the expected JS / TS binding shape.

**Implementation Steps**:
1. **Rewrite `wasm/Cargo.toml`.**
2. **Author the wrapper `lib.rs`** by inventorying the previous public wasm-bindgen API (grep `#[wasm_bindgen]` history before move, or check `wasm/pkg/` from the last successful build).
3. **Implement `JsCallbacks`.**
4. **`wasm-pack build`** until clean.
5. **Spot-check `wasm/pkg/` exports** match the previous shape (function names, parameter counts).

**Dependencies**: Phase 3.

**Testing Approach**:
- Integration: `wasm-pack build wasm/ --target web` succeeds.
- Static: `wasm/pkg/` contains expected exports (manual inspection).

**Acceptance Criteria**:
- [ ] `wasm-pack build wasm/ --target web` exits 0.
- [ ] `wasm/pkg/` is produced.
- [ ] `wasm/src/` contains only `lib.rs` (and optionally any small helper module if the wrapper grows).

**Estimated Effort**: medium.

---

### Phase 5: TS / Tauri compatibility verification

**Goal**: The existing TypeScript code and Tauri build operate unchanged with the new thin wrapper.

**Files to Modify**:
- `src/**/*.ts` — only if the wasm-pack output shape changed in a way that breaks imports. Default expectation: zero changes.

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| TS import sites | Consume `wasm/pkg/` | Phase 4 done | Behaviour unchanged |

**Processing Flow**:
1. Run `bun install` if dependencies need refresh.
2. Run `bun run wasm:build` (or whatever path the project uses), confirm pkg/ is recreated.
3. Run `bun tauri build`.
4. Run `bun test`.
5. Smoke-run `bun tauri dev` and confirm a shell session renders.

**Implementation Steps**:
1. **`bun run wasm:build`** if not chained to Phase 4 already.
2. **`bun tauri build`** and resolve any import path drift.
3. **`bun test`** and address regressions.
4. **`bun tauri dev`** smoke check.

**Dependencies**: Phase 4.

**Testing Approach**:
- Integration: `bun tauri build`, `bun test` both exit 0.
- Manual: `bun tauri dev` renders an interactive shell.

**Acceptance Criteria**:
- [ ] `bun tauri build` exits 0.
- [ ] `bun test` exits 0.
- [ ] Manual sanity check: `bun tauri dev` shows the terminal and accepts input.

**Estimated Effort**: small.

---

### Phase 6: native-poc switch to term_core

**Goal**: `native-poc` no longer ships its own parser/grid; it imports `term_core` via a workspace path dependency. Its pre-existing tests (selection, pty input, etc.) still pass.

**Files to Modify**:
- `native-poc/Cargo.toml` — add `term_core = { path = "../crates/term_core" }`.
- `native-poc/src/tabs.rs` — switch the grid/parser types to term_core's.
- `native-poc/src/render/mod.rs` — read term_core grid snapshots.
- `native-poc/src/window_host.rs` — adjust grid mutex / events accordingly.

**Files to Delete**:
- `native-poc/src/parser/` (entire directory).
- `native-poc/src/grid/` (entire directory).

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| native-poc Tab | Owns a term_core Parser + Grid | term_core API available | Tab uses term_core types end-to-end |
| native-poc Renderer | Reads term_core Grid via snapshot or accessor | Tab refactored | Frames render from term_core data |
| native-poc tests | Selection, key encoding, pty round-trip | Refactor compiles | Existing tests pass |

**Processing Flow**:
1. Add term_core dependency in `native-poc/Cargo.toml`.
2. Delete `native-poc/src/parser/` and `native-poc/src/grid/`.
3. Update `tabs.rs`, `render/mod.rs`, `window_host.rs` to use term_core types.
4. Reconcile any signature drift (e.g., callback shape if PoC implemented its own minimal one).
5. Run `cargo build --manifest-path native-poc/Cargo.toml`.
6. Run `cargo test --manifest-path native-poc/Cargo.toml`.

**Implementation Steps**:
1. **Add dependency, delete stand-in modules.**
2. **Rewire Tab and render** to use term_core types.
3. **Implement `TerminalCallbacks` for native-poc** (minimal: log-only or simple title hook is fine for Phase 2; Phase 3 will flesh it out).
4. **Build and test** until green.

**Dependencies**: Phase 2 (term_core), and ideally Phase 3 so that term_core's API is test-stable.

**Testing Approach**:
- Integration: `cargo build --manifest-path native-poc/Cargo.toml` exits 0.
- Integration: `cargo test --manifest-path native-poc/Cargo.toml` exits 0 with at least the previous test count (selection, pty input, pty round trip).

**Acceptance Criteria**:
- [ ] `native-poc/src/parser/` and `native-poc/src/grid/` are gone.
- [ ] `native-poc/Cargo.toml` depends on `term_core` via path.
- [ ] `cargo build` for native-poc exits 0.
- [ ] `cargo test` for native-poc exits 0; pre-existing test count is preserved.

**Estimated Effort**: medium.

---

### Phase 7: Final workspace verification and cleanup

**Goal**: A single end-to-end pass of every command listed in `sdd.yaml` confirms the migration. Documentation reflects the new structure.

**Files to Modify**:
- `native-poc/README.md` — note dependency on `term_core`.
- (Optional) `crates/term_core/README.md` — one-paragraph crate description.

**Processing Flow**:
1. From a clean target dir, run:
   - `cargo fmt --all`
   - `cargo build --workspace`
   - `cargo test --workspace`
   - `cargo clippy --workspace --no-deps`
2. From the project root:
   - `bun run wasm:build` (if separate)
   - `bun tauri build`
   - `bun test`
3. Aggregate results.

**Implementation Steps**:
1. **Format.**
2. **Workspace-wide build + test + clippy.**
3. **TS-side build + test.**
4. **Update README files** as needed.

**Dependencies**: Phases 1-6.

**Testing Approach**:
- Integration: all commands listed in `sdd.yaml` exit 0.

**Acceptance Criteria**:
- [ ] `cargo fmt --all --check` exits 0.
- [ ] `cargo build --workspace` exits 0.
- [ ] `cargo test --workspace` exits 0.
- [ ] `cargo clippy --workspace --no-deps` exits 0 (warnings tolerated; deny on must-fix).
- [ ] `bun tauri build` exits 0.
- [ ] `bun test` exits 0.

**Estimated Effort**: small.

---

## Complete File Structure

```
emterm/
├── Cargo.toml                          # NEW: workspace root
├── crates/
│   └── term_core/                      # NEW: pure Rust crate
│       ├── Cargo.toml
│       └── src/
│           ├── lib.rs
│           ├── callbacks.rs            # rewritten: TerminalCallbacks trait
│           ├── parser/
│           │   ├── mod.rs / state.rs / ground.rs / escape.rs / csi.rs / osc.rs / dcs.rs / apc.rs
│           │   └── tests.rs
│           ├── csi_*.rs
│           ├── terminal_*.rs
│           ├── sgr.rs / slim_cell.rs / ring_buffer.rs / reflow.rs / snapshot.rs
│           ├── char_table.rs / style_table.rs / color_spec.rs
│           ├── esc_handler.rs / osc_handler.rs / apc_handler.rs
│           ├── c0_handler.rs / print_handler.rs
│           └── parser_params.rs / parser_types.rs / bench.rs
├── wasm/                               # REWRITTEN as thin wrapper
│   ├── Cargo.toml                      # depends on term_core
│   └── src/
│       └── lib.rs                      # wasm-bindgen wrapper + JsCallbacks
├── src/                                # unchanged (TypeScript)
├── src-tauri/                          # unchanged
├── native-poc/                         # parser/ and grid/ removed
│   ├── Cargo.toml                      # adds term_core dep
│   └── src/
│       ├── (existing files minus parser/, grid/)
│       └── (tabs / render / window_host rewired to term_core)
└── doc/tasks/term-core-rust-crate/     # this SDD
```

## Testing Strategy

- **Unit tests**: 100% of pre-migration parser/grid/sgr/ring_buffer tests run under `cargo test -p term_core`.
- **Integration tests**: workspace-wide `cargo build` and `cargo test`; wasm-pack build; `bun tauri build`; `bun test`.
- **Manual**: `bun tauri dev` smoke check.

## Dependencies

| Package | Version | Purpose |
|---------|---------|---------|
| serde / serde_derive | 1 | Same as today; used by term_core |
| unicode-width | 0.2 | Same as today |
| log | 0.4 | Same as today |
| bincode | 1.3 | Conditional — kept if a native consumer uses it; otherwise wasm-side only |
| wasm-bindgen | 0.2 | Thin wrapper only |
| js-sys | 0.3 | Thin wrapper only |
| web-sys | 0.3 | Thin wrapper only |
| serde-wasm-bindgen | 0.6 | Thin wrapper only |
| console_error_panic_hook | latest | Thin wrapper only (optional) |

Exact versions are pinned via the existing `Cargo.lock` and the workspace-shared `Cargo.lock` after Phase 1.

## Risk Assessment

| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|------------|
| wasm-bindgen-strip surface larger than expected (>15k LOC) | medium | medium | Three-pass mechanical strip; revisit per file after grep |
| Tests dependent on JsValue can't be ported cleanly | low | medium | Rewrite to call Rust API; log every drop in VERIFICATION_RESULT.md |
| `wasm-pack` output drifts (function names, return types) | medium | high | Inventory previous exports before Phase 4; compare `wasm/pkg/` JS/d.ts after build |
| TS side breaks despite intent of keeping exports identical | low | high | Phase 5 verification gate explicitly runs `bun tauri build` and `bun test` |
| Workspace introduces edition or feature flag conflicts | medium | low | Establish in Phase 1 before any move; iterate until clean |
| `bincode` placement ambiguous | low | low | Decide during Phase 2 based on actual usage; default to thin wrapper |
| native-poc TerminalCallbacks impl mismatch | medium | low | Provide a minimal impl (log-only) in Phase 6; Phase 3 of the full project will flesh it out |

## Open Questions

- [ ] term_core's Rust edition (assumed: keep 2024; downgrade to 2021 only if workspace forces it).
- [ ] Final `TerminalCallbacks` trait surface (assumed: 1:1 with existing callbacks.rs).
- [ ] Whether `bincode` belongs in term_core (assumed: thin wrapper unless a native consumer surfaces a need).
- [ ] Whether to bump any of the dependency versions during the migration (assumed: no; YAGNI).

## Success Metrics

- [ ] All Phase 1-7 acceptance criteria met.
- [ ] No test silently dropped.
- [ ] `bun tauri build` is green at every phase boundary from Phase 5 onwards.
- [ ] `native-poc` parser/grid directories are removed and the binary still tests green.
