# Verification Document: block-cursor-glyph-font

## Overview

**Feature**: block-cursor-glyph-font
**SPEC.md**: `feature-docs/block-cursor-glyph-font/SPEC.md`
**IMPLEMENTATION.md**: `feature-docs/block-cursor-glyph-font/IMPLEMENTATION.md`

## Build Verification

- Command: `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml`
- Expected: exit code 0, no errors
- Additional: `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml --no-default-features`
  - Expected: exit code 0, no errors (CLI-only build unaffected — the fix
    is inside `#[cfg(feature = "gui")]` render code, so the CLI feature
    surface should be untouched).

## Test Verification

- Command: `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib -- --test-threads=1`
- Expected: pre-existing `render::cursor` tests plus new tests
  introduced by task0001 (AC-1..AC-6) pass. Known-flaky tests unrelated
  to this feature (e.g. `tabs.rs` off-thread replay) may fail
  intermittently; treat as non-blocking if the same test also fails on
  base_commit.
- Coverage target: no numeric target; each Acceptance Criterion in
  task0001 has ≥ 1 mapped test as its "done" signal.

### Test Scenarios from SPEC.md

| ID | Scenario | Expected Result | Test Type |
|----|----------|-----------------|-----------|
| TS-1 | Block cursor over an existing ASCII glyph resolves through the shared font module | Same cache key / raster as the grid pass for the same code point | Unit |
| TS-2 | Block cursor over a code point where `cursor_glyph_paintable` is false | Rect drawn, no glyph raster requested | Unit |
| TS-3 | Overlay glyph color plumbing | Equals `resolve_cell_style_from_packed(...).bg` for the covered cell (reverse video / selection / dim / hidden respected) | Unit |
| TS-4 | Wide (2-cell) glyph under block cursor | `block_cursor_rect` covers 2 cells; glyph lookup fires exactly once at the leading column; existing `resolve_cursor_glyph_col` snap behavior intact | Unit |
| TS-5 | Second consecutive resolve of the same glyph | Reuses the cache entry; no new `egui::TextureHandle` allocated | Unit |
| TS-6 | Preedit / underline / bar / hollow-block cursor rendering | Unchanged output; existing tests stay green with no edits | Unit (existing) |

## Code Quality Verification

- Format: driven by the project's PostToolUse hook (per-file rustfmt);
  no crate-wide `cargo fmt` per project convention. Verification is
  implicit — the hook runs automatically on file writes.
- Static analysis: `cargo check` above is the primary static gate.

## SPEC.md Compliance

### Success Criteria

| ID | Criterion | How to Verify |
|----|-----------|---------------|
| SC-1 | Inconsolata `0` under block cursor renders with slashed-zero shape matching grid | MT-1 manual visual check |
| SC-2 | ASCII / CJK / symbols all match grid glyph identity | Unit test TS-1 for identity, MT-1 for eyeball check on `0O1lI` + a CJK sample |
| SC-3 | Wide-glyph regression-free | TS-4 unit + MT-1 eyeball check on a CJK line |
| SC-4 | Non-block cursor styles regression-free | TS-6 unit + MT-1 eyeball check with cursor style toggled |
| SC-5 | Rust tests pass, typecheck passes, CLI-only build passes | Build + test verification above; `bun run typecheck` + `bun test` as unrelated smoke check |
| SC-6 | Doc comment updated for new glyph path | Review inspection (spec-compliance reviewer) |

### Functional Requirements Coverage

| Requirement | Tasks | Verification |
|-------------|-------|--------------|
| FR1 | task0001 | TS-1, AC-5 (no FontId::monospace on the covered-glyph path) |
| FR2 | task0001 | TS-1 identity check + MT-1 visual |
| FR3 | task0001 | TS-3 |
| FR4 | task0001 | TS-4 |
| FR5 | task0001 | TS-2 |
| FR6 | task0001 | TS-1 (fallback chain is the shared one) + MT-2 Windows visual |
| NFR1 | task0001 | TS-5 (no per-frame texture alloc) + MT-3 informal CPU spot check |
| NFR2 | task0001 | AC-7 (grid instance data unchanged) — inspection + terminal_grid_pass tests green |
| NFR3 | task0001 | MT-2 Windows visual (if a Windows build is available) |
| NFR4 | task0001 | Review inspection (spec-compliance reviewer) |

## E2E Testing

No project-wide E2E harness is configured for the native renderer
(chrome-devtools MCP does not attach to the wgpu surface, and
tauri-driver is not set up here). E2E scenarios reduce to manual
visual checks in the Manual Testing section below.

## Manual Testing (E2E Not Possible)

- [ ] MT-1: Launch `src-tauri/target-host/release/emterm` with the
      terminal font set to Inconsolata. Print `0O1lI` and a short CJK
      line to the shell. Move the cursor across each cell with arrow
      keys. Expected: every character under the block cursor keeps the
      same glyph shape as when the cursor is elsewhere (slashed-zero,
      etc.).
- [ ] MT-2: Repeat MT-1 on a Windows build if available (per SPEC
      NFR3). If no Windows build is available in this cycle, defer with
      an explicit note in the verify-phase result — does NOT block
      Linux completion.
- [ ] MT-3: While a shell is idle, watch `top` or the render-cpu-
      optimization benchmark output with the cursor blinking on a
      filled cell. Expected: no perceptible CPU regression versus a
      base_commit build.
- [ ] MT-4: Toggle cursor style to underline, bar, and unfocused
      (window blur). Expected: no visual change from the pre-fix
      behavior in those states.

## Performance / Security Verification

- NFR1 threshold: no numeric target; a qualitative "no perceptible
  regression" spot check via MT-3 is sufficient. Not a completion
  blocker unless a clear regression is observed.
- No security surface — pure rendering change.

## Verification Summary

| Category | Items | Automated | E2E | Manual |
|----------|-------|-----------|-----|--------|
| Build | 2 | 2 | 0 | 0 |
| Tests | 6 (TS-1..TS-6) | 6 | 0 | 0 |
| Success Criteria | 6 (SC-1..SC-6) | 3 | 0 | 3 |
| FR / NFR coverage | 10 | 6 | 0 | 4 |
| Manual | 4 (MT-1..MT-4) | 0 | 0 | 4 |
