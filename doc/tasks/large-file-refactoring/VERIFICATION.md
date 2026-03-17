# Verification Document: Large File Decomposition Refactoring

## Overview
**Feature**: large-file-refactoring
**SPEC.md**: `doc/tasks/large-file-refactoring/SPEC.md`
**IMPLEMENTATION.md**: `doc/tasks/large-file-refactoring/IMPLEMENTATION.md`

## Build Verification

### TypeScript
- Command: `bun run typecheck`
- Expected: exit code 0, no type errors

### Rust Backend
- Command: `cargo build --manifest-path src-tauri/Cargo.toml`
- Expected: exit code 0, no compilation errors

### WASM
- Command: `cd wasm && wasm-pack build`
- Expected: exit code 0, WASM module builds successfully

## Test Verification

### TypeScript Tests
- Command: `bun test`
- Expected: All existing tests pass, no regressions

### Rust Backend Tests
- Command: `cargo test --manifest-path src-tauri/Cargo.toml`
- Expected: All existing tests pass

### WASM Tests
- Command: `cd wasm && cargo test`
- Expected: All existing tests pass

### Test Scenarios from SPEC.md

| ID | Scenario | Expected Result | Test Type |
|----|----------|-----------------|-----------|
| TS-01 | All existing TypeScript unit tests pass | Zero failures | Unit (automated) |
| TS-02 | TypeScript typecheck passes | Zero type errors | Integration (automated) |
| TS-03 | All existing Rust tests pass | Zero failures | Unit (automated) |
| TS-04 | All existing WASM tests pass | Zero failures | Unit (automated) |
| TS-05 | E2E tests pass without regression | All specs green | E2E Docker (automated) |
| TS-06 | No circular imports introduced | No runtime errors, no bundler warnings | Integration (automated) |
| TS-07 | External import paths unchanged | No import errors in consuming files | Integration (automated) |

## Code Quality Verification

### Format
- Rust: `cargo fmt --manifest-path src-tauri/Cargo.toml --check` and `cd wasm && cargo fmt --check`
- Expected: No formatting issues

### Static Analysis
- TypeScript: `bun run typecheck` (strict mode)
- Rust: `cargo clippy --manifest-path src-tauri/Cargo.toml` and `cd wasm && cargo clippy`
- Expected: No new warnings

## File Structure Verification

### Files to Create (18 new files)

**Phase 1 — terminal-app:**
- `src/terminal-app/pty-handler.ts` — PTY data flow orchestration functions
- `src/terminal-app/osc-handler.ts` — OSC callback dispatch functions
- `src/terminal-app/resize-handler.ts` — Resize handling functions
- `src/terminal-app/ui-handler.ts` — UI event handling functions

**Phase 2 — canvas-renderer:**
- `src/terminal/renderer-line.ts` — Line rendering functions
- `src/terminal/renderer-decorations.ts` — Decoration drawing functions
- `src/terminal/renderer-cursor.ts` — Cursor rendering functions
- `src/terminal/renderer-selection.ts` — Selection rendering functions
- `src/terminal/renderer-fold.ts` — Fold rendering functions
- `src/terminal/renderer-settings.ts` — Settings application functions

**Phase 3 — state:**
- `src/terminal/state-buffer.ts` — Buffer switching functions
- `src/terminal/state-wasm-sync.ts` — WASM synchronization functions
- `src/terminal/state-actions.ts` — Action processing functions
- `src/terminal/state-response.ts` — Response management functions

**Phase 4 — unified-buffer:**
- `src/terminal/buffer-scroll.ts` — Scroll operation functions

**Phase 5 — layer:**
- `src/image/layer-placement.ts` — Image placement calculation functions

**Phase 6 — terminal_core (Rust):**
- `wasm/src/terminal_cells.rs` — Cell accessor impl block
- `wasm/src/terminal_rows.rs` — Row operation impl block

### Files to Modify (7 existing files)
- `src/terminal-app/index.ts` — Delegate to extracted handlers, add re-exports
- `src/terminal/canvas-renderer.ts` — Delegate to renderer modules, add re-exports
- `src/terminal/state.ts` — Delegate to state modules, add re-exports
- `src/terminal/unified-buffer.ts` — Delegate to buffer-scroll, add re-exports
- `src/image/layer.ts` — Delegate to layer-placement, add re-exports
- `wasm/src/terminal_core.rs` — Add mod declarations, remove moved impl blocks
- `wasm/src/lib.rs` — Add mod declarations if needed

## SPEC.md Compliance

### Success Criteria

| ID | Criterion | How to Verify |
|----|-----------|---------------|
| SC-01 | All 6 target files under 1000 lines | `wc -l` on each file |
| SC-02 | All existing tests pass | Run full test suite (TS + Rust + WASM) |
| SC-03 | TypeScript typecheck passes | `bun run typecheck` exits 0 |
| SC-04 | No external import path changes | Grep for imports of target modules across codebase, verify no changes needed |
| SC-05 | No functional changes | E2E tests pass, diff shows only structural moves |
| SC-06 | 18 new focused modules created | File existence check |

### Functional Requirements Coverage

| Requirement | Phase | Verification |
|-------------|-------|--------------|
| FR1: terminal-app/index.ts decomposition | Phase 1 | `wc -l src/terminal-app/index.ts` < 1000; 4 new files exist; bun test passes |
| FR2: canvas-renderer.ts decomposition | Phase 2 | `wc -l src/terminal/canvas-renderer.ts` < 1000; 6 new files exist; bun test passes |
| FR3: state.ts decomposition | Phase 3 | `wc -l src/terminal/state.ts` < 1000; 4 new files exist; bun test passes |
| FR4: unified-buffer.ts decomposition | Phase 4 | `wc -l src/terminal/unified-buffer.ts` < 1000; 1 new file exists; bun test passes |
| FR5: layer.ts decomposition | Phase 5 | `wc -l src/image/layer.ts` < 1000; 1 new file exists; bun test passes |
| FR6: terminal_core.rs decomposition | Phase 6 | `wc -l wasm/src/terminal_core.rs` < 1000; 2 new files exist; cargo test passes |
| NFR1: Backward Compatibility | All | No import path changes in consuming files |
| NFR2: Performance | All | E2E tests show no rendering regressions |
| NFR3: Code Quality | All | Each new module under 400 lines (guideline) |

## E2E Testing (Docker)

- Command: `./scripts/run-e2e-docker.sh test`
- [ ] All existing E2E specs pass without modification
- [ ] Terminal renders correctly (text, cursor, selection, images)
- [ ] No visual regressions in screenshots

## Re-export Parity Verification

Before starting refactoring, capture the baseline export symbols:
- Run `grep -n '^export' src/terminal-app/index.ts src/terminal/canvas-renderer.ts src/terminal/state.ts src/terminal/unified-buffer.ts src/image/layer.ts` to record all exported symbols
- After each phase, verify the same symbols are still exported from the same paths
- TypeScript typecheck (`bun run typecheck`) will catch most missing re-exports via import errors in consuming files
- [ ] All pre-refactoring exported symbols are still accessible from original file paths

## Manual Testing (E2E Not Possible)

- [ ] Visual code review: verify split modules have clear, cohesive responsibilities (not mechanical line-count splits)

## Performance Verification

- No performance requirements beyond "zero regression"
- Module splitting resolved at build time by Bun bundler — zero runtime overhead
- Rust module splitting resolved at compile time — zero runtime overhead

## Security Verification

Not applicable (internal refactoring only, no new attack surface).

## Line Count Verification

After completion, verify each target file:

| File | Before | Target |
|------|--------|--------|
| `src/terminal-app/index.ts` | 1425 | < 1000 |
| `src/terminal/canvas-renderer.ts` | 1895 | < 1000 |
| `src/terminal/state.ts` | 1442 | < 1000 |
| `src/terminal/unified-buffer.ts` | 1154 | < 1000 |
| `src/image/layer.ts` | 1080 | < 1000 |
| `wasm/src/terminal_core.rs` | 1536 (824 impl) | < 1000 |

## Verification Summary

| Category | Items | Automated | E2E (Docker) | Manual |
|----------|-------|-----------|--------------|--------|
| Build | 3 | 3 | 0 | 0 |
| Unit Tests | 3 | 3 | 0 | 0 |
| Type Check | 1 | 1 | 0 | 0 |
| Code Format | 2 | 2 | 0 | 0 |
| Static Analysis | 2 | 2 | 0 | 0 |
| File Structure | 22 | 22 | 0 | 0 |
| Line Count | 6 | 6 | 0 | 0 |
| Functional | 9 | 7 | 1 | 1 |
| **Total** | **48** | **46** | **1** | **1** |
