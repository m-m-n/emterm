# Verification Document: per-tab-grid-size

## Overview

**Feature**: per-tab-grid-size /
**SPEC.md**: `feature-docs/per-tab-grid-size/SPEC.md` /
**IMPLEMENTATION.md**: `feature-docs/per-tab-grid-size/IMPLEMENTATION.md`

This documents the INTEGRATED verification run by the verify phase.
Task-level acceptance criteria live in `tasks/task0001.md` /
`tasks/task0002.md`.

## Build Verification

- Command (main / GUI):
  `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml`
- Command (CLI feature-gate, NFR3):
  `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml --no-default-features`
- Expected: exit code 0, no errors, for both.

## Test Verification

- Command:
  `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib -- --test-threads=1`
  (Rust tests for this project live under `--lib`; the tabs.rs replay tests
  are non-deterministic in parallel, hence `--test-threads=1`.)
- Coverage target: no numeric coverage tooling is configured for this
  project; the coverage criterion is full scenario coverage — TS1-TS6
  automated, TS7 manual.

### Test Scenarios from SPEC.md

| ID | Scenario | Expected Result | Test Type |
|----|----------|-----------------|-----------|
| TS1 | Grid-size change with multiple tabs open (app.rs) | Only the active tab's core dims change; an inactive tab's core reports its prior dims | Unit |
| TS2 | Tab activation with differing vs matching stored size (app.rs) | Differing: resized exactly at activation. Matching: no resize issued. Close-tab / exited-reap activation fix-ups reconcile the same way | Unit |
| TS3 | Wire-domain clamp on initial spawn and every later resize (tabs.rs) | Existing clamp tests (`resize_clamps_to_the_wire_domain_before_resizing_the_core`, `spawn_shell_clamps_the_initial_core_to_the_wire_domain`) and the app-side clamp-record test pass; core dims always post-clamp | Unit (existing) |
| TS4 | Mux `Resize` control-frame scoping (app.rs, compositional) | An inactive mux tab's core dims stay unchanged across a grid-size change (no `Tab::resize` invocation ⇒ no frames, `Tab::resize` being the sole emission site per the pinned contract) | Unit |
| TS5 | Per-tab reflow invalidation (app.rs + tabs.rs) | Width-changing resize clears the resized tab's prompt/fold marks (tab side) and the app selection/pending anchor (app side); height-only change and untouched tabs keep all trackers | Unit |
| TS6 | Full regression | The complete `--lib` suite passes with the command above | Integration |
| TS7 | 3-tab leak reproduction (2026-08-03 scenario) | No XTWINOPS response fragment (`;R;Ct` form, e.g. `;51;171t816;1368t`) appears in the tmux shell | Manual |

## Code Quality Verification

- Format: no format command is configured (`format_command` empty). Do NOT
  run a crate-wide `cargo fmt` — the project is intentionally not
  rustfmt-clean; formatting stays local to edited lines.
- Static analysis: none configured; the two build commands above are the
  compile gates.

## SPEC.md Compliance

### Success Criteria

| ID | Criterion | How to Verify |
|----|-----------|---------------|
| SC1 | Each tab holds its own grid size independently | TS1 |
| SC2 | Tab switch / window resize / UI visibility changes never change an inactive tab's PTY size | TS1, TS7 |
| SC3 | 3-tab mux/tmux/normal setup: mux→tmux switch leaks no XTWINOPS fragment | TS7 |
| SC4 | Toggling mux status bar / sidebar Persistent does not propagate resize to hidden tabs | TS7 |
| SC5 | Existing tests pass or are updated with justification | TS3, TS6 + justification comments in updated tests |
| SC6 | CLI build stays green (`--no-default-features`) | Build Verification (CLI command) |

### Functional Requirements Coverage

| Requirement | Tasks | Verification |
|-------------|-------|--------------|
| FR1 | task0001, task0002 | TS1 |
| FR2 | task0001 | TS1, TS7 |
| FR3 | task0001 | TS2 |
| FR4 | task0001, task0002 | TS4 |
| FR5 | task0001, task0002 | TS3 |
| FR6 | task0001, task0002 | TS5 |
| NFR1 | task0001 | TS7 |
| NFR2 | task0001, task0002 | TS3, TS6 |
| NFR3 | task0001, task0002 | TS6 + CLI build check |

## E2E Testing

No E2E infrastructure exists in this project (test/README.md); the
end-to-end leak criterion is verified manually as TS7 below.

## Manual Testing (E2E Not Possible)

- [ ] **TS7 — 3-tab leak reproduction**:
  1. Build and launch the GUI binary with sidebar Persistent mode ON and the
     mux status bar ON.
  2. Open 3 tabs: one mux session, one running tmux, one plain shell.
  3. Switch tabs repeatedly, in particular mux → tmux.
  4. Toggle the mux status bar and sidebar Persistent mode while a hidden
     tab hosts tmux, then switch back to it.
  5. Confirm no `;R;Ct`-form fragment (e.g. `;51;171t816;1368t`) ever
     appears in the tmux-hosted shell, and hidden tabs come back at their
     own size, reconciled only at activation.

(The design step was skipped for this feature — there is no mockup
comparison item.)

## Performance / Security Verification

Not applicable — no performance or security requirement is specified
(SPEC.md; the change restricts which PTYs receive TIOCSWINSZ and relocates
in-process state only).

## Verification Summary

| Category | Items | Automated | E2E | Manual |
|----------|-------|-----------|-----|--------|
| Build | 2 (GUI check, CLI check) | 2 | 0 | 0 |
| Unit / regression tests | TS1-TS6 | 6 | 0 | 0 |
| Manual scenario | TS7 | 0 | 0 | 1 |
| Total | 9 | 8 | 0 | 1 |
