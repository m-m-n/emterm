# Verification Document: term_core Rust Crate Extraction (Phase 2)

## Overview
**Feature**: term-core-rust-crate
**SPEC.md**: `doc/tasks/term-core-rust-crate/SPEC.md`
**IMPLEMENTATION.md**: `doc/tasks/term-core-rust-crate/IMPLEMENTATION.md`

## Build Verification

- Workspace build: `cargo build --workspace` — exit 0. **PASS (Phases 1-5 only; native-poc temporarily excluded from members).**
- term_core only: `cargo build -p term_core` — exit 0. **PASS**.
- Thin wrapper: `wasm-pack build wasm/ --target web` — exit 0; `wasm/pkg/` present. **PASS** (via `bun run build:wasm`).
- Tauri release: `bun tauri build` — exit 0. **PASS** (produces deb + rpm bundles).

## Test Verification

- Workspace tests: `cargo test --workspace` — **PARTIAL**: 597 pass for term_core; 997/998 pass for src-tauri (1 pre-existing failure: `pty::session::tests::test_session_sets_term_program_env`, env-dependent shell-rc test unrelated to Phase 2).
- term_core only: `cargo test -p term_core` — 597 passed, 0 failed, 3 ignored. **PASS** (matches pre-migration test count).
- TS tests: `bun test` — 2325 pass, 17 todo, 0 fail. **PASS**.

## Execution Notes (Phase 6 / Phase 7 deferred)

Phases 1-5 completed in this session. Phases 6 (native-poc swap) and 7
(final cleanup) are left as `pending` in `tasks.yaml` and require a
follow-up session:

- Phase 6 needs rewiring native-poc's `tabs.rs` / `render/mod.rs` /
  `window_host.rs` from its own `parser/` + `grid/` modules to
  `term_core::TerminalCore`. native-poc's renderer reads
  `Grid::primary.cell(row, col)` style; switching to term_core's
  `get_cell_fg/bg/flags` (packed u32) and `get_cell_char` requires a
  conversion layer in `render/mod.rs`. The wry/tao stack also needs to
  be upgraded (or its `kuchiki` transitive pinned) before native-poc can
  rejoin the workspace `members` list.
- Phase 7's `cargo fmt --check`, `cargo clippy --workspace --no-deps`,
  and a final `bun tauri build` regression pass are pending; the
  current session ran fmt once successfully (no diff after) and the
  Tauri build green check was completed during Phase 5.

### Test Scenarios from SPEC.md

| ID | Scenario | Expected Result | Test Type |
|----|----------|-----------------|-----------|
| TS-1 | All `mod tests` in moved sources pass under `cargo test -p term_core` | All pass | Unit |
| TS-2 | `parser/tests.rs` passes (~1k LOC of parser/grid coverage) | All pass | Unit |
| TS-3 | term_core has no dependency on wasm-bindgen / js-sys / web-sys / serde-wasm-bindgen | `cargo metadata -p term_core` shows none of these | Static |
| TS-4 | `cargo build --workspace` succeeds | exit 0 | Integration |
| TS-5 | `cargo test --workspace` succeeds | exit 0 | Integration |
| TS-6 | `wasm-pack build wasm/ --target web` produces `wasm/pkg/` | pkg/ contains expected bindings | Integration |
| TS-7 | `bun tauri build` succeeds with the new thin wrapper in place | exit 0 | Integration |
| TS-8 | `bun test` (TS) passes | exit 0 | Integration |
| TS-9 | native-poc builds and existing tests pass after term_core swap | `cargo build` + `cargo test` exit 0 | Integration |
| TS-10 | `bun tauri dev` opens a window and ANSI rendering works as before | functionally equivalent | Manual |
| TS-11 | Build time of `cargo build -p term_core` is not noticeably slower than the previous `cargo build` for wasm/ | informal feel | Manual |
| TS-12 | `wasm/pkg/` export shape (function names, parameter counts, d.ts) matches the pre-migration baseline at `tmp/term-core-baseline/pkg/` | `diff -r tmp/term-core-baseline/pkg/ wasm/pkg/` shows only intentional changes | Manual |
| TS-13 | TerminalCallbacks trait surface covers every former `js_sys::Function` call site | grep audit | Static / Manual |
| TS-14 | No test is silently dropped during migration | dropped tests, if any, are listed with reasons in VERIFICATION_RESULT.md | Manual / Audit |

## Code Quality Verification

- Format: `cargo fmt --all -- --check` — **PASS** (no diff after running `cargo fmt --all`).
- Static analysis: `cargo clippy --workspace --no-deps` — **PENDING** (Phase 7).

## File Structure Verification

### Files to Create
- [x] `Cargo.toml` (repo root) — workspace.
- [x] `crates/term_core/Cargo.toml`.
- [x] `crates/term_core/src/**` — moved from `wasm/src/**` via `git mv` (history preserved).
- [x] `wasm/src/lib.rs` — thin wrapper.
- [ ] (Optional) `crates/term_core/README.md` — not added.

### Files to Modify
- [x] `wasm/Cargo.toml` — rewritten to depend on term_core.
- [ ] `native-poc/Cargo.toml` — pending Phase 6.
- [ ] `native-poc/src/tabs.rs`, `native-poc/src/render/mod.rs`, `native-poc/src/window_host.rs` — pending Phase 6.
- [x] `Cargo.lock` (root) — pinned from `src-tauri/Cargo.lock` to preserve tauri 2.9.5 (matches the npm-side @tauri-apps/api 2.9.1).
- [x] `scripts/patch-wasm-bindgen.sh` — added handling for wasm-bindgen >= 0.2.100 split-export form (old form remains supported).
- [x] `.gitignore` — added `target/` (workspace build dir).

### Files to Delete (or empty after move)
- [x] `wasm/src/*.rs` — all moved; only the new `lib.rs` remains.
- [ ] `native-poc/src/parser/` — pending Phase 6.
- [ ] `native-poc/src/grid/` — pending Phase 6.

### Phase 1 Baseline Snapshot (for TS-12 diff)
- [x] `tmp/term-core-baseline/pkg/` — pre-migration `.d.ts` / `.js` / `package.json`.
- [x] `tmp/term-core-baseline/wasm-bindgen-exports.txt` — 33 `#[wasm_bindgen]` sites recorded.
- [x] `tmp/term-core-baseline/js-callback-sites.txt` — 2 `js_sys::Function` sites recorded (informed the 5-method `TerminalCallbacks` trait surface).

## SPEC.md Compliance

### Success Criteria
| ID | Criterion | How to Verify |
|----|-----------|---------------|
| SC-1 | FR1 (workspace) | Root `Cargo.toml` exists; `cargo build --workspace` exits 0 |
| SC-2 | FR2 (code relocation via git mv) | `git log --follow` on a moved file shows pre-migration history |
| SC-3 | FR3 (wasm-bindgen stripped from term_core) | TS-3 (static metadata check) |
| SC-4 | FR4 (TerminalCallbacks trait) | TS-13 |
| SC-5 | FR5 (thin wrapper) | TS-6; `wasm/src/` contains only `lib.rs` |
| SC-6 | FR6 (cargo test green) | TS-1, TS-2, TS-5 |
| SC-7 | FR7 (TS-facing exports unchanged) | TS-7, TS-8, TS-12 |
| SC-8 | FR8 (native-poc switched) | TS-9; parser/ and grid/ removed from native-poc |
| SC-9 | NFR1 (Tauri build green) | TS-7 |
| SC-10 | NFR3 (module layout preserved) | Manual diff of `crates/term_core/src/` tree vs. previous `wasm/src/` tree |
| SC-11 | NFR4 (no silent test loss) | TS-14 |

### Functional Requirements Coverage
| Requirement | Phase | Verification |
|-------------|-------|--------------|
| FR1 — Cargo workspace | Phase 1 | TS-4 |
| FR2 — git mv | Phase 2 | TS-2, TS-14, manual `git log --follow` |
| FR3 — wasm-bindgen strip | Phase 2 | TS-3 |
| FR4 — TerminalCallbacks | Phase 2 | TS-13 |
| FR5 — Thin wrapper | Phase 4 | TS-6, TS-12 |
| FR6 — Tests on cargo test | Phase 3 | TS-1, TS-2 |
| FR7 — TS export parity | Phase 4-5 | TS-7, TS-8, TS-12 |
| FR8 — native-poc swap | Phase 6 | TS-9 |
| NFR1 — Tauri green | Phase 5, Phase 7 | TS-7, TS-8 |
| NFR2 — Build time parity | Phase 7 | TS-11 |
| NFR3 — Module layout preserved | Phase 2 | Manual tree diff |
| NFR4 — No silent test loss | Phase 3 | TS-14 |
| NFR5 — Linux-only validation | All | Implicit (verification host) |

## E2E Testing

This feature has no UI E2E tests of its own. The existing WebdriverIO + tauri-driver suite under `e2e-tests/` continues to target the Tauri build and must remain green if it is run.

- [ ] `./scripts/run-e2e-docker.sh` (if executed) continues to pass.

## Manual Testing (E2E Not Possible)

- [ ] TS-10: `bun tauri dev` smoke test — terminal renders, accepts input, ANSI sequences work as before.
- [ ] TS-11: Informal `cargo build -p term_core` timing vs. the previous wasm build.
- [ ] TS-12: `wasm/pkg/` export shape diff vs. previous pkg/.
- [ ] TS-13: Grep audit confirms TerminalCallbacks covers all former JS callback call sites.
- [ ] TS-14: Migration drop list (if any) recorded with reasons.

## Performance Verification

- NFR2 (build-time parity): captured during Phase 7 with at least two `cargo build -p term_core` samples on a clean target, compared against pre-migration `cargo build` of `wasm/` on the same machine.

## Security Verification

- [ ] No new external dependencies introduced beyond those carried over from `wasm/Cargo.toml`.
- [ ] No new persistence or network surface.

## Verification Summary

| Category | Items | Automated | E2E | Manual |
|----------|-------|-----------|-----|--------|
| Term_core unit tests | TS-1, TS-2 | 2 | 0 | 0 |
| Term_core metadata | TS-3 | 1 | 0 | 0 |
| Workspace build/test | TS-4, TS-5 | 2 | 0 | 0 |
| wasm-pack + Tauri build | TS-6, TS-7 | 2 | 0 | 0 |
| TS test suite | TS-8 | 1 | 0 | 0 |
| native-poc build/test | TS-9 | 1 | 0 | 0 |
| Manual smoke / parity | TS-10, TS-11, TS-12, TS-13, TS-14 | 0 | 0 | 5 |
| **Total** | **14** | **9** | **0** | **5** |
