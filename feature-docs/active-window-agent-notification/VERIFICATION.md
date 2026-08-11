# Verification Document: active-window-agent-notification

## Overview

**Feature**: active-window-agent-notification
**SPEC.md**: `feature-docs/active-window-agent-notification/SPEC.md`
**IMPLEMENTATION.md**: `feature-docs/active-window-agent-notification/IMPLEMENTATION.md`

This documents the integrated, feature-wide verification run by the verify
phase. Task-level acceptance criteria live in `tasks/task0001.md` and
`tasks/task0002.md`.

## Build Verification

Commands from workflow.yaml `project.components`, byte-for-byte. Expected:
exit code 0, no errors.

| Component | Command |
|-----------|---------|
| rust | `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml` |
| app_settings (CLI-only feature gate) | `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml --no-default-features` |
| typescript | `bun run build:viewer && bun run build:settings` |

## Test Verification

| Component | Command |
|-----------|---------|
| rust | `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib` |
| app_settings | `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path crates/app_settings/Cargo.toml --lib` |
| typescript | `bun test` |
| typescript_types | `bun run typecheck` |

- Coverage target: no numeric coverage gate is configured for this project;
  coverage is scenario-driven — every scenario below must be represented by a
  passing test (or listed check).

### Test Scenarios from SPEC.md

TS-1 … TS-7 are the scenarios mapped in workflow.yaml `requirements`; TS-8 and
TS-9 are added by this plan to close the FR6 / NFR4 coverage gaps.

| ID | Scenario | Expected Result | Test Type |
|----|----------|-----------------|-----------|
| TS-1 | `should_fire_agent_notification` combinations: (pane_visible × new setting ON/OFF) × (blocked/done) × master/global/event-type toggles | Fires only when the visibility condition (not visible OR setting ON) and every existing gate pass; non-visible outcome identical for setting ON/OFF | Unit (rust) |
| TS-2 | Clear transition (no new state) with the new setting ON | Never a notification candidate | Unit (rust) |
| TS-3 | Rate-limit sharing: a visible-pane fire, then a second qualifying transition in the same pane within 30 s | Second transition suppressed by the shared per-pane rate limit | Unit (rust) |
| TS-4 | `app_settings` serde: missing key / null / explicit `false` for `agent_notify_visible_pane` | Missing and null resolve to `true`; explicit `false` round-trips | Unit (app_settings) |
| TS-5 | `agent-section.test.ts`: the new toggle renders after the existing three, saves the exact key, en/ja labels resolve | Render / save / i18n assertions pass following the existing test pattern | Integration (bun test) |
| TS-6 | `bun run typecheck` | TS `AppSettings` mirror is consistent; no type errors | Type check |
| TS-7 | CLI build (`cargo check --no-default-features`) | Compiles — `app_settings` stays an always-built crate | Build check |
| TS-8 | Tab-activity notification regression (output / bell / process-exit) | Existing tab-activity gating tests are unmodified and pass — their focus/visibility gates are untouched (FR6) | Regression (rust unit suite) |
| TS-9 | Drain loop with notifications disabled (existing `pump_all` drains-when-off test) | Transition queue is drained every pump even while the settings are off — no unbounded growth (NFR4) | Unit (rust) |

## Code Quality Verification

- Format: no `format_command` is configured for any component (workflow.yaml)
  — skipped.
- Static analysis: covered by the build commands above (compiler checks) and
  `bun run typecheck`.

## SPEC.md Compliance

### Success Criteria

| ID | Criterion | How to Verify |
|----|-----------|---------------|
| SC-1 | New setting ON (default): blocked/done in the focused window's active tab fires a desktop notification | TS-1 (gate level) + manual M-1 (real desktop notification) |
| SC-2 | New setting OFF: visible-pane notifications suppressed as before; non-visible panes unaffected | TS-1 + manual M-2 |
| SC-3 | Agent section toggle changes the behaviour and persists to `settings.json` (en/ja) | TS-5 + manual M-3 |
| SC-4 | Existing master / global / event-type toggles and per-pane 30 s rate limit still suppress | TS-1, TS-3 |
| SC-5 | `settings.json` lacking the key, or with it null, resolves to default ON | TS-4 |
| SC-6 | Tab activity notifications (output / bell / process-exit) unchanged | TS-8 |
| SC-7 | CLI build (`--no-default-features`) still compiles | TS-7 |

### Functional Requirements Coverage

| Requirement | Tasks | Verification |
|-------------|-------|--------------|
| FR1 | task0001 | TS-1, TS-2 |
| FR2 | task0001 | TS-1, TS-4 |
| FR3 | task0002 | TS-6 |
| FR4 | task0002 | TS-5 |
| FR5 | task0001 | TS-1, TS-3 |
| FR6 | task0001 | TS-8 |
| NFR1 | task0001, task0002 | TS-4, TS-5, TS-6 |
| NFR2 | task0001 | TS-1 (gate tests run headless, GUI-free) |
| NFR3 | task0001 | TS-7 |
| NFR4 | task0001 | TS-9 |

## E2E Testing

No E2E framework is configured for this project (workflow.yaml
`e2e_test_command` is empty for every component; SPEC.md: existing E2E tests —
none). Desktop-notification end-to-end behaviour is covered by the manual
section below.

## Manual Testing (E2E Not Possible)

Desktop notifications require a real OS notification service and a focused
window, which the automated suites cannot drive.

- [ ] M-1: With default settings, run an agent in the focused window's active
      tab; on a blocked/done transition a desktop notification appears.
- [ ] M-2: Turn the new Agent-section toggle OFF; a blocked/done transition in
      the visible pane no longer notifies, while a background tab's pane
      still does.
- [ ] M-3: Flip the toggle, restart eMterm, and confirm the value persisted
      (`settings.json` contains `agent_notify_visible_pane`); labels display
      correctly in both en and ja.
- [ ] M-4: Sanity: output / bell / process-exit tab-activity notifications
      behave exactly as before.

(The design step was skipped for this feature — no mockup visual comparison
applies.)

## Performance / Security Verification

- NFR4 (resource use): TS-9 verifies the transition queue drains
  unconditionally while notifications are disabled.
- Security: not applicable per SPEC.md (no network/database surface; the only
  persisted value is a boolean; the toggle reuses the existing `renderToggle`
  markup path).

## Verification Summary

| Category | Items | Automated | E2E | Manual |
|----------|-------|-----------|-----|--------|
| Build checks | 3 | 3 | 0 | 0 |
| Unit / integration scenarios (TS-1..TS-5, TS-8, TS-9) | 7 | 7 | 0 | 0 |
| Type / build scenarios (TS-6, TS-7) | 2 | 2 | 0 | 0 |
| Manual scenarios (M-1..M-4) | 4 | 0 | 0 | 4 |
