# Verification Document: mux Window Close Redraw

## Overview
**Feature**: mux-window-close-redraw
**SPEC.md**: `doc/tasks/mux-window-close-redraw/SPEC.md`
**IMPLEMENTATION.md**: `doc/tasks/mux-window-close-redraw/IMPLEMENTATION.md`

## Build Verification
- Command: `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml`
- CLI-only gate: `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml --no-default-features`
- Expected: exit code 0, no errors or new warnings in `tabs.rs`.
- **Result (2026-06-19)**: PASS. Default-feature `cargo check` exit 0, no
  warnings. CLI-only (`--no-default-features`) `cargo check` exit 0 — the change
  is entirely within the GUI-gated `tabs` module (NFR2).

## Test Verification
- Command: `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --bin emterm`
- Coverage target: the new close-reconcile decision and the `PtyExited` arm
  paths (active-changed / active-unchanged / emptied / unknown-pane).
- **Result (2026-06-19)**: PASS. The unit tests live in the lib target, so the
  suite was run as `cargo test --lib`:
  - `tabs::` module: 79/79 passed (includes the 6 new close-reconcile tests and
    the existing `pty_exited_*` / `switch_window_*` regression tests).
  - Full lib suite single-threaded (`-- --test-threads=1`): 1820 passed, 0
    failed, 1 ignored (pre-existing).
  - Note: under default parallelism the off-thread snapshot-replay worker tests
    (`swap_replaces_outgoing_content`, `ts5_queued_live_output_applied_in_order`,
    `ts9_no_residual_rows_after_offthread_swap_to_shorter_pane`) intermittently
    time out under contention. Verified pre-existing: the same (and more)
    timeouts occur on the unmodified baseline; they pass in isolation and
    single-threaded. Unrelated to this change.

### Test Scenarios from SPEC.md
| ID | Scenario | Expected Result | Test Type |
|----|----------|-----------------|-----------|
| TS-1 | Active window's shell exits in a 3-window group | Decision helper returns the now-active pane id (a snapshot reconcile is requested) | Unit |
| TS-2 | A non-active window's shell exits | Decision helper returns "none" (no reconcile requested); active window unchanged | Unit |
| TS-3 | The last remaining window's shell exits | Tab marked `exited`; decision helper returns "none" (no reconcile requested) | Unit |
| TS-4 | `PtyExited` for an unknown pane id | No removal, no reconcile, arm reports no change | Unit |
| TS-5 | Several `PtyExited` for distinct panes drain in one pump | Final active window is the one reconciled | Unit |
| TS-6 | Regression: inbound `SwitchWindow` still reconciles | Active index synced and the now-active window drawn (unchanged behavior) | Unit |

## Code Quality Verification
- Format: `CARGO_TARGET_DIR=src-tauri/target cargo fmt --manifest-path src-tauri/Cargo.toml`
- Static analysis: standard `cargo check` warnings (no clippy gate in this repo).
- **Result (2026-06-19)**: PASS. Formatting scoped to the touched file —
  `rustfmt --edition 2024 --check src-tauri/src/tabs.rs` exit 0 (the repo's
  PostToolUse format hook applied edition-2024 style on save). No crate-wide
  `cargo fmt` run. `cargo check` produced no new warnings.

## File Structure Verification
### Files to Create
- (none) — confirmed, no files created.

### Files to Modify
- [x] `src-tauri/src/tabs.rs` — `PtyExited` arm of `Tab::apply_mux_message`, the
  `Tab::close_reconcile_target` decision helper, and 6 unit tests. `git diff
  --stat` shows this is the only modified source file (NFR3: no `src/` WebView
  change).

## SPEC.md Compliance

### Success Criteria
| ID | Criterion | How to Verify |
|----|-----------|---------------|
| SC-1 | After a shell-exit close, only the now-active window's content is shown | Manual scenario (no overlap) + TS-1 |
| SC-2 | The correction happens without a manual switch | Manual scenario |
| SC-3 | Existing mux switch / close-tab behavior unchanged | TS-3, TS-6 + existing mux test suite green |
| SC-4 | CLI-only build still compiles | `--no-default-features` cargo check |

### Functional Requirements Coverage
| Requirement | Phase | Verification |
|-------------|-------|--------------|
| FR1 | Phase 1 | TS-1, TS-5, manual scenario |
| FR2 | Phase 1 | TS-2 |
| FR3 | Phase 1 | TS-3 |
| NFR1 | Phase 1 | TS-6 + existing mux tests |
| NFR2 | Phase 1 | `--no-default-features` cargo check |
| NFR3 | Phase 1 | diff touches only `src-tauri/`, not `src/` |

## Manual Testing (E2E Not Possible)
This project has no E2E framework; the visual outcome is human-judged.
- [ ] Open a mux tab with 3 windows; run distinguishable output in each.
- [ ] Make one window active, exit its shell (`exit` / Ctrl+D).
- [ ] Confirm a different window becomes active and shows only its own content —
      no overlap with the closed window — without switching away and back.
- [ ] Exit shells until only one window remains, then exit it; confirm the tab
      closes and `mux kill` is not blocked.
- [ ] Exit the shell of a non-active window; confirm the visible window is
      unchanged.

## Verification Summary
| Category | Items | Automated | E2E | Manual |
|----------|-------|-----------|-----|--------|
| Build | 2 | 2 | 0 | 0 |
| Unit tests | 6 | 6 | 0 | 0 |
| Manual scenarios | 4 | 0 | 0 | 4 |
| Format/static | 1 | 1 | 0 | 0 |
