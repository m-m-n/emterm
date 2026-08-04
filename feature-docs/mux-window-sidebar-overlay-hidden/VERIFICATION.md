# Verification Document: mux-window-sidebar-overlay-hidden

## Overview

**Feature**: mux-window-sidebar-overlay-hidden
**SPEC.md**: `feature-docs/mux-window-sidebar-overlay-hidden/SPEC.md`
**IMPLEMENTATION.md**: `feature-docs/mux-window-sidebar-overlay-hidden/IMPLEMENTATION.md`

## Build Verification

- Command: `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml`
- Feature-gate check (CLI-only build must stay green): `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml --no-default-features`
- Expected: exit code 0, no errors

## Test Verification

- Command: `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib`
  - Pre-existing, unrelated flakiness in `tabs.rs` replay tests may require
    a rerun with `-- --test-threads=1`; that flakiness is not a failure of
    this feature.
- Integration regression: `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --test cli_subcommands`
- Coverage target: no numeric coverage tooling is configured for this
  project. Criterion instead: the new attach-transition branch is exercised
  by dedicated unit tests (TS1: positive startup case, reattach case, and
  the steady-attached negative case), and the existing detach-guard branch
  keeps its dedicated regression tests (TS2).

### Test Scenarios from SPEC.md

| ID | Scenario | Expected Result | Test Type |
|----|----------|-----------------|-----------|
| TS1 | Pump sequence where the active tab's mux attach state goes absent→present (startup attach; reattach after explicit close; plus the steady-attached negative case) | `mux_sidebar_overlay_open` reads open after the transition pump; a user-closed flag stays closed while continuously attached | Unit (inline in `src-tauri/src/app.rs`) |
| TS2 | Existing detach-guard and overlay test group (`ac7_*`, teardown reset, tab-switch survival, toggle round-trip/no-op) | All pass unmodified — attached→not-attached still resets the flag to closed | Unit (regression) |
| TS3 | Full library suite | All tests pass (single-threaded rerun allowed for the pre-existing `tabs.rs` replay flakiness) | Unit/Integration (regression) |
| TS4 | Launch via `~/bin/init-mux` with `window_sidebar_overlay: true`; then Ctrl+S close → detach → reattach | Overlay sidebar visible immediately at startup; reopens after reattach | Manual |

## Code Quality Verification

- Format: rustfmt is enforced by the project's PostToolUse hook on touched
  files. Do NOT run crate-wide `cargo fmt` (project policy — pre-existing
  style drift in unrelated files); formatting is verified on the changed
  file only.
- Static analysis: the build verification commands above (`cargo check`
  default-features and `--no-default-features`) serve as the static check;
  no separate linter is configured in workflow.yaml.

## SPEC.md Compliance

### Success Criteria

| ID | Criterion | How to Verify |
|----|-----------|---------------|
| SC1 | Immediately after the detach guard (verified location `app.rs:3979-3986` at planning time — SPEC's `3922-3929` drifted; anchor by code shape per IMPLEMENTATION.md D1), a pump with the not-attached→attached transition assigns the overlay-open flag | Code review of the diff + TS1 |
| SC2 | Launching via init-mux with `window_sidebar_overlay: true` shows the sidebar from startup | TS4 (manual) + TS1 (unit proxy) |
| SC3 | After explicit Ctrl+S close, detach → reattach returns the sidebar to open | TS4 (manual) + TS1 reattach unit test |
| SC4 | A new test asserts the flag is open after launch → mux attach completion | TS1 exists and passes |
| SC5 | Existing detach-guard tests (`ac7_*` group and related) pass without regression | TS2 + TS3 |

### Functional Requirements Coverage

| Requirement | Tasks | Verification |
|-------------|-------|--------------|
| FR1 | task0001 | TS1 (attach-transition unit tests), TS2 (detach guard unchanged) |
| FR2 | task0001 | TS1 (unit proxy for startup attach), TS4 (manual startup check) |
| FR3 | task0001 | TS1 (reattach unit test), TS4 (manual reattach check) |
| NFR1 | task0001 | TS2, TS3 (regressions green), plus diff inspection: only `src-tauri/src/app.rs` changed |
| NFR2 | task0001 | TS3, plus diff inspection: no settings-schema/persistence change; flag stays runtime-only |

## E2E Testing

No E2E framework exists for this project (SPEC.md: "Existing E2E tests:
None"). Section intentionally omitted in favor of manual testing below.

## Manual Testing (E2E Not Possible)

- [ ] TS4-a: With `settings.mux.window_sidebar_overlay: true`, launch emterm
      via `~/bin/init-mux`. The floating overlay sidebar card is visible as
      soon as the mux attach completes, with no toggle pressed.
- [ ] TS4-b: Close the sidebar with prefix + Ctrl+S, detach from the mux
      session, reattach. The sidebar is open again.
- [ ] Known accepted behaviors to NOT report as defects during manual
      testing (IMPLEMENTATION.md D2): the reattach reopen after an explicit
      close (that is FR3 itself), and a reopen when switching the active tab
      from a non-mux tab to a mux-attached tab.

## Performance / Security Verification

Not applicable — single runtime boolean assignment in the pump loop; no
performance, security, input-handling, or persistence surface.

## Verification Summary

| Category | Items | Automated | E2E | Manual |
|----------|-------|-----------|-----|--------|
| Build | 2 (default + no-default-features check) | 2 | 0 | 0 |
| Unit / regression tests | 3 (TS1, TS2, TS3) | 3 | 0 | 0 |
| Manual scenarios | 2 (TS4-a, TS4-b) | 0 | 0 | 2 |
| Success criteria | 5 (SC1-SC5) | 4 | 0 | 1 (SC2/SC3 manual halves) |
