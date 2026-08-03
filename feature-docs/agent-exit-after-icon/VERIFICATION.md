# Verification Document: agent-exit-after-icon

## Overview

**Feature**: agent-exit-after-icon
**SPEC.md**: `feature-docs/agent-exit-after-icon/SPEC.md`
**IMPLEMENTATION.md**: `feature-docs/agent-exit-after-icon/IMPLEMENTATION.md`

## Build Verification

- Command: `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml`
- Expected: exit code 0, no errors

## Test Verification

- Command: `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib`
- Coverage target: every Acceptance Criterion across task0001-task0004 has
  a passing test (no numeric coverage percentage target — this project
  has no coverage tooling; see `test/README.md`)

### Test Scenarios from SPEC.md

| ID | Scenario | Expected Result | Test Type |
|----|----------|-----------------|-----------|
| TS-1 | Core latch: `Set` → live `D` → live `A` | Exactly one inferred-clear signal | Unit |
| TS-2 | Core latch / GUI / daemon: `Set` → live `A` only (no `D`) | No inferred-clear signal | Unit + Integration |
| TS-3 | Core latch / GUI / daemon: `Set` → explicit `Clear` → live `D` → live `A` | No second/duplicate clear | Unit + Integration |
| TS-4 | Core latch: `Set` → live `D` → `Set` (new generation) → live `A` | No inferred-clear signal (old `D` invalidated) | Unit |
| TS-5 | Core latch: no `Set` ever → live `D` → live `A` | No-op, no signal | Unit |
| TS-6 | Plain tab integration: full flow through `AgentStatusModel` | `state: None` after `Set`→`D`→`A` | Integration |
| TS-7 | Mux pane integration: full flow through the daemon | `state: None`, revision incremented, waiter resolved, `AgentStatusUpdate(state: None)` pushed | Integration |
| TS-8 | Snapshot/replay-sourced OSC 133 marks (GUI and daemon) | Latch never fires from replay-derived marks | Integration |
| TS-9 | Alt-screen-suppressed OSC 133 marks (GUI and daemon) | Latch never fires from alt-screen marks | Integration |
| TS-10 | Mux hot-upgrade occurring between a live `D` and its matching `A` | Latch state survives the upgrade; inferred clear still fires post-upgrade when the `A` arrives | Integration |
| TS-11 | Nested subshell inside a pane emits a bare `A` with no preceding post-`Set` `D` | No clear (D→A ordering guard holds); documented as a partial mitigation, not a complete guarantee | Integration |
| TS-12 | Core latch: `Set` → live `D` → live `D` again (repeated) → live `A` | Exactly one inferred-clear signal (repeated `D` does not multiply or break state) | Unit |
| TS-13 | Plain tab whose shell never emits OSC 133 | Icon remains until an explicit `Clear`, unchanged from pre-feature behavior | Integration |
| TS-14 | Mux pane whose shell never emits OSC 133 | Icon remains until an explicit `Clear`, unchanged from pre-feature behavior | Integration |

## Code Quality Verification

- Format: no dedicated `format_command` configured for this project
  (`project.components.main.format_command` is empty — formatting is
  enforced by the project's PostToolUse hook per its formatting policy,
  not a standalone verify-phase command).
- Static analysis: covered by the Build Verification command above
  (`cargo check`).

## SPEC.md Compliance

### Success Criteria

| ID | Criterion | How to Verify |
|----|-----------|----------------|
| SC-1 | FR1-FR6 implemented and tested | TS-1 through TS-12 all pass |
| SC-2 | All test scenarios pass | `cargo test --lib` exit code 0 |
| SC-3 | Behavior symmetric between plain tabs and mux panes | TS-6 and TS-7 both pass with equivalent outcomes |
| SC-4 | Panes without OSC 133 support show unchanged behavior | task0002 AC-7 and task0003 AC-6 (NFR3 regression guards) pass |
| SC-5 | Code review completed | review phase `residual_critical_high: 0` |

### Functional Requirements Coverage

| Requirement | Tasks | Verification |
|-------------|-------|---------------|
| FR1 | task0001 | TS-1, TS-2, TS-4, TS-5, TS-12 |
| FR2 | task0002, task0003 | TS-6, TS-7 (inferred clear applied via the existing explicit-Clear path) |
| FR3 | task0002, task0003 | TS-6, TS-7 (daemon-authoritative for mux panes; symmetric plain-tab behavior) |
| FR4 | task0002, task0003 | TS-3 (no reordering-induced double clear), TS-11 |
| FR5 | task0002, task0003 | TS-8, TS-9 |
| FR6 | task0004 | TS-10 |
| NFR1 | task0002, task0003 | TS-3 (explicit Set/Clear semantics unchanged by the addition) |
| NFR2 | task0001 | Design-level (O(1) fixed-size latch state, no new hot-loop scans — checked by review, not a runtime test) |
| NFR3 | task0002, task0003 | TS-13, TS-14 |

## E2E Testing

No E2E framework exists in this repository (no `e2e-tests/`,
`tests/e2e/`, `docker-compose.e2e.yml`, `playwright.config.*`,
`cypress.config.*` detected). This feature does not introduce one.

## Manual Testing (E2E Not Possible)

- [ ] Start eMterm with a real shell configured to emit OSC 133 (e.g.
  starship prompt). Report `working` via the `emterm agent-status`
  CLI (or equivalent), Ctrl+C, and confirm the tab/status-bar icon
  clears without a manual `clear` call — both on a plain tab and inside
  a mux pane.
- [ ] Repeat with a shell that does NOT emit OSC 133 and confirm the
  icon still requires an explicit clear (no regression).

## Performance / Security Verification

Not applicable per SPEC.md (no throughput/latency-sensitive path
introduced; no security-relevant surface touched).

## Verification Summary

| Category | Items | Automated | E2E | Manual |
|----------|-------|-----------|-----|--------|
| Unit | TS-1, TS-2, TS-4, TS-5, TS-12 | 5 | 0 | 0 |
| Integration | TS-2, TS-3, TS-6, TS-7, TS-8, TS-9, TS-10, TS-11, TS-13, TS-14 | 10 | 0 | 0 |
| Manual | shell-with-OSC133 / shell-without-OSC133 checks | 0 | 0 | 2 |
