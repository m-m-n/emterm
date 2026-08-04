# Verification Document: mux-status-bar-removal

## Overview

**Feature**: mux-status-bar-removal /
**SPEC.md**: `feature-docs/mux-status-bar-removal/SPEC.md` /
**IMPLEMENTATION.md**: `feature-docs/mux-status-bar-removal/IMPLEMENTATION.md`

## Build Verification

- Rust build check:
  `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml`
- Feature-gate check (NFR1):
  `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml --no-default-features`
- TypeScript bundles:
  `bun run build:viewer && bun run build:settings`
- Expected: exit code 0, no errors, for all three.

## Test Verification

- Rust unit suites (src-tauri `--lib`, plus the app_settings and mux_ipc
  workspace crates):
  `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib`
- Rust integration (hot-upgrade, serialized):
  `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --test mux_hot_upgrade -- --test-threads=1`
- TypeScript: `bun test` and `bun run typecheck`
- Coverage target: no numeric coverage target for this removal feature —
  the criterion is that every suite above passes (NFR2) and the scenario
  table below is covered by dedicated tests.

### Test Scenarios from SPEC.md

| ID | Scenario | Expected Result | Test Type |
|----|----------|-----------------|-----------|
| TS1 | runtime.rs: view model / OSC row without mux input | `build_view_model` no longer takes/uses mux status; OSC row renders only from the OSC 777 dispatcher; row count unchanged by mux attach | Unit |
| TS2 | tabs.rs / app.rs: raw frame with retired opcode 0x16 from a stale daemon | Ignored with at most a warn log; tab state undisturbed (replaces `on_mux_message_status_update_caches_payload_on_tab`) | Unit |
| TS3 | app_settings + src-tauri settings loader: JSON containing `mux.statusbar` | Deserializes without error; obsolete key ignored | Unit |
| TS4 | window_host.rs: inset / grid-size candidates | Driven only by general status-bar visibility; no mux-conditional path remains | Unit |
| TS5 | mux/ipc: relocated `detect_osc7_cwd`; pane cwd across hot-upgrade | Relocated tests pass; `Pane.cwd` updates from OSC 7 and survives hot-upgrade (mux_hot_upgrade green, `--test-threads=1`) | Unit + Integration |
| TS6 | TypeScript mirror without `MuxStatusbarSettings` | `bun test` and `bun run typecheck` pass with types.ts and fixtures updated | Unit (TS) |
| TS7 | 3-tab mux/tmux/plain: switch mux→tmux | Inactive tmux tab's PTY is not resized; no XTWINOPS response text leaks into the tmux screen | Manual |

## Code Quality Verification

- Format / static analysis: no format command is declared in workflow.yaml
  `project.components`; formatting is enforced by the project's own hooks —
  no separate verification step here.
- Symbol hygiene (AC-1): repository-wide search for `MuxStatusbarSettings`,
  `StatusUpdateMsg`, `mux_status_state`, `StatusBarEngine` returns no hits
  outside the reserved-opcode comment and historical docs.

## SPEC.md Compliance

### Success Criteria

| ID | Criterion | How to Verify |
|----|-----------|---------------|
| AC-1 | No mux status bar code/state/settings remain (four-symbol search clean) | Repository-wide search (Symbol hygiene above); statusbar.rs deleted |
| AC-2 | Non-sidebar functions dispositioned: pane cwd retained, templates/commands intentionally retired | TS5 (cwd retained); schema removal verified by TS3/TS6; retirement is by design (assumption A2) — no migration check |
| AC-3 | Grid rows identical with/without mux attach; provably mux-independent in unit tests | TS1 + TS4 |
| AC-4 | settings.json with populated `mux.statusbar` loads without error | TS3 |
| AC-5 | All builds and suites in NFR1/NFR2 green | Build Verification + Test Verification commands above |

### Functional Requirements Coverage

| Requirement | Tasks | Verification |
|-------------|-------|--------------|
| FR1 | task0001 | TS1, TS2 |
| FR2 | task0001 | AC-1 symbol search + build/test gates (no dedicated scenario) |
| FR3 | task0001 | AC-1 symbol search + retired-opcode decode tolerance test in mux_ipc + build/test gates (no dedicated scenario ID) |
| FR4 | task0001, task0002 | TS3 (Rust mirrors), TS6 (TS mirror) |
| FR5 | task0001 | TS1, TS4; TS7 (manual) |
| FR6 | task0001 | TS1, TS4; osc_dispatcher/callbacks untouched (task0001 AC-6) |
| FR7 | task0001 | TS5 |
| FR8 | task0001 | TS2 (0x16 at GUI; daemon-side 0x17 counterpart in task0001 AC-3), TS3 |
| NFR1 | task0001 | Feature-gate check command (Build Verification) |
| NFR2 | task0001, task0002 | All Test Verification commands green |
| NFR3 | task0001 | Existing sidebar / tab-bar / agent-status tests unmodified and green; no diff outside the task file sets |

## E2E Testing

No E2E framework command is declared for this feature in workflow.yaml —
section omitted.

## Manual Testing (E2E Not Possible)

- [ ] TS7 — In a 3-tab mux/tmux/plain setup, switching mux→tmux no longer
      resizes the inactive tmux tab's PTY and no XTWINOPS response text
      leaks into the tmux screen (`tmp/discussion-mux-tab-switch-leak.md`
      scenario). Manual/user verification — not run in automated verify;
      record as a manual verification note. Full root-cause elimination of
      per-tab grid coupling stays in the separate follow-up task.

## Performance / Security Verification

Not applicable — no performance or security requirements are defined for
this feature (REQUIREMENTS.md 5.4).

## Verification Summary

| Category | Items | Automated | E2E | Manual |
|----------|-------|-----------|-----|--------|
| Unit tests (Rust) | TS1, TS2, TS3, TS4, TS5(unit) | 5 | 0 | 0 |
| Integration tests (Rust) | TS5 (mux_hot_upgrade) | 1 | 0 | 0 |
| TypeScript | TS6 (bun test + typecheck) | 1 | 0 | 0 |
| Build checks | cargo check, --no-default-features, bun builds | 3 | 0 | 0 |
| Symbol hygiene | AC-1 four-symbol search | 1 | 0 | 0 |
| Manual | TS7 | 0 | 0 | 1 |
