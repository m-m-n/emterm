# Verification Document: tmux-startup-query-response-leak

## Overview

**Feature**: tmux-startup-query-response-leak
**SPEC.md**: `feature-docs/tmux-startup-query-response-leak/SPEC.md`
**IMPLEMENTATION.md**: `feature-docs/tmux-startup-query-response-leak/IMPLEMENTATION.md`

This feature has no E2E infrastructure available (project-wide fact, see
`test/README.md`). Verification is split honestly: the leak MECHANISM is
verified by automated Rust `--lib` tests; the on-screen BEHAVIOR (TS1–TS3)
is verified manually by the user on a release build.

## Build Verification

- Command: `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml`
- CLI feature gate (TS6): `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml --no-default-features`
- Expected: exit code 0, no errors, no new warnings.

## Test Verification

- Command: `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib`
- Deflaked re-run (use when the `tabs.rs` replay tests flake in parallel):
  `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib -- --test-threads=1`
- term_core suite: `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path crates/term_core/Cargo.toml --lib`
- Coverage target: no numeric coverage tooling is enforced in this project.
  The coverage criterion is enumerative: every in-scope response type
  (DA1 / DA2 / DSR status / CPR / XTWINOPS 14, 16, 18 / DECRPM) has at
  least one test per in-scope runtime context (plain tab, mux pane),
  plus the single-chunk multi-query burst case (TS8) and the off-thread
  replay-discard invariant (TS9) added in review-round-1 rework.

### Test Scenarios from SPEC.md

| ID | Scenario | Expected Result | Test Type |
|----|----------|-----------------|-----------|
| TS1 | Manual, plain tab: run `tmux` in a fresh tab of the release build; observe the pane during and after startup | No stray sequences (`^[[>65;1;0c`, `^[[8;R;Ct`, `^[[4;H;Wt` or similar) visible at or after the prompt | Manual |
| TS2 | Manual, mux pane: attach to a mux session, run `tmux` inside a pane; then detach and reattach | Clean startup; no replayed leak after reattach | Manual |
| TS3 | Manual, tmux health: inside the started tmux, split panes, resize the window, check colors and status line | tmux behaves normally — responses were routed, not suppressed | Manual |
| TS4 | Unit: feed each in-scope query sequence through the plain-tab and mux-pane parse paths | Response delivered toward the querying PTY writer exactly once; never present in the visible grid or scrollback; includes the failing-before/passing-after root-cause reproduction test | Unit |
| TS5 | Unit, negative: query/response-lookalike bytes embedded in ordinary application output | Bytes reach the grid unchanged; non-query sessions byte-identical (NFR3) | Unit |
| TS6 | Feature-gate check | `--no-default-features` check exits 0 (NFR2) | Build gate |
| TS7 | Regression: pre-existing per-tab-grid-size and mux-snapshot-device-query-strip tests | All green, unmodified (or modified only with justification comments), in the `--lib` runs above | Unit |
| TS8 | Unit, multi-query burst (rework round 1): a SINGLE parse chunk / coalesced buffer carrying multiple distinct in-scope queries (at minimum DA1, DA2, XTWINOPS 14/16/18) driven through the term_core chunk path, the plain-tab combined path, and the mux-pane path | Every response delivered toward the querying PTY exactly once, in query order; none visible in grid or scrollback; a second drain returns empty | Unit |
| TS9 | Unit, off-thread swap replay discard (rework round 1): a worker-built core replaying snapshot bytes that embed an in-scope query, applied through the off-thread core-swap path | No replay-generated response bytes delivered toward the PTY after the swap (parity with the synchronous replay discard); a query arriving after the swap is answered exactly once | Unit |

## Code Quality Verification

- Format: `cargo fmt --manifest-path src-tauri/Cargo.toml --check`
  — the tree carries known pre-existing drift on main; the criterion is NO
  NEW drift relative to the base commit, not a clean exit.
- TypeScript components: not applicable — the task's file set contains no
  TypeScript; `bun` checks are not required for this feature.

## SPEC.md Compliance

### Success Criteria

| ID | Criterion | How to Verify |
|----|-----------|---------------|
| SC1 | Fresh-tab tmux start leaves no query-response text visible | TS1 (manual) |
| SC2 | tmux functions normally after startup (colors, resize, status line) | TS3 (manual) |
| SC3 | Every in-scope response type exercised by ≥1 automated test; full `--lib` suite passes | TS4 + test-run commands above |
| SC4 | `--no-default-features` check passes | TS6 |
| SC5 | Both in-scope runtime contexts verified | TS1 + TS2 (manual) and TS4's per-context tests (automated) |
| SC6 | per-tab-grid-size tab-switch leak does not recur | TS7 |

### Functional Requirements Coverage

| Requirement | Tasks | Verification |
|-------------|-------|--------------|
| FR1 | task0001, task0003 | TS1 (manual on-screen), TS4 (automated mechanism), TS9 (off-thread replay discard) |
| FR2 | task0001, task0002, task0003 | TS3 (manual tmux health), TS4 (exactly-once delivery assertions), TS8 (multi-query burst exactly-once), TS9 (no stale replay delivery) |
| FR3 | task0001, task0002, task0003 | TS4, TS8, TS9 (inline `#[cfg(test)]` tests exist and run under `--lib`) |
| FR4 | task0001, task0002 | TS1 (plain tab) + TS2 (mux pane) manual; per-context automated tests in TS4 and TS8. Status: assumed → `both` |
| FR5 | task0001, task0002 | TS4 enumerates DA1 / DA2 / DSR status / CPR / XTWINOPS 14, 16, 18 / DECRPM; TS8 exercises the in-scope set as a single-chunk burst. Status: assumed → `generalize` |
| FR6 | task0001 | TS7 (per-tab-grid-size suite green) |
| NFR1 | task0001 | TS4 plus the implementer's hot-path reasoning (AC-7); if the strip predicate changed, its `#[ignore]` 2 MiB bench re-run manually under its documented threshold |
| NFR2 | task0001 | TS6 |
| NFR3 | task0001 | TS5 |
| NFR4 | task0001 | TS7 (mux-snapshot-device-query-strip suite green) |

## E2E Testing

None — the project has no E2E infrastructure (`test/README.md`). The
runtime-context scenarios are covered manually below.

## Manual Testing (E2E Not Possible)

Performed by the user on a release build
(`src-tauri/target-host/release/emterm`; rebuild is user-initiated, never
run unprompted):

- [ ] TS1 — plain tab: launch eMterm, open a fresh tab, run `tmux`;
      observe the pane during and after startup. No stray escape-sequence
      text at or after the tmux/shell prompt.
- [ ] TS2 — mux pane: attach to a mux session, run `tmux` inside a pane;
      confirm clean startup; detach, reattach, confirm no replayed leak.
- [ ] TS3 — tmux health: inside the started tmux, split panes, resize the
      window, verify colors and status line render correctly (responses
      routed, not suppressed).

## Performance / Security Verification

- NFR1 (performance): non-query PTY traffic gains no per-byte work —
  verified in review against IMPLEMENTATION.md D4 (per-frame/chunk
  classification only), plus the optional strip-predicate bench above.
- Security: no auth/input-validation/data-protection surface (SPEC.md).

## Verification Summary

| Category | Items | Automated | E2E | Manual |
|----------|-------|-----------|-----|--------|
| Build gates | 2 (check, no-default-features) | 2 | 0 | 0 |
| Unit / regression | TS4, TS5, TS7, TS8, TS9 | 5 | 0 | 0 |
| On-screen behavior | TS1, TS2, TS3 | 0 | 0 | 3 |
| Code quality | fmt no-new-drift | 1 | 0 | 0 |
| Performance | NFR1 review + optional bench | 0 | 0 | 1 |
