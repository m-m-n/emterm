# Verification Document: agent-notify-visible-pane-docs

## Overview

**Feature**: agent-notify-visible-pane-docs / **SPEC.md**: `feature-docs/agent-notify-visible-pane-docs/SPEC.md` / **IMPLEMENTATION.md**: not produced (tier `reduced`, single task with no same-file overlap)

Documentation-only change. The integrated verification is a set of document
checks (text search, diff listing, and reading against the code). Build and
test commands are listed as a regression guard only: TS-5 establishes that no
compiled or bundled input changed.

All commands run from the integration worktree root. "Base revision" below
means the feature's implement base commit (`workflow.implement.base_commit`
in workflow.yaml); when it is not recorded, use the merge-base of the
integration branch and `main`.

Section names used below:

- "Notifications section" = the level-2 section titled Notifications in
  `doc/AGENT-STATUS.md`, from its heading to the next level-2 heading or the
  end of the file.
- "Agent Status section" = the level-4 section titled "Mux Agent Status and
  Agent API" in `doc/SPECIFICATION.md`.

## Build Verification

- Command (main): `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml`
- Command (web): `bun run build:viewer && bun run build:settings`
- Expected: exit code 0, no errors (regression guard; no source input changed per TS-5)

## Test Verification

- Command (main): `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib`
- Command (web): `bun test`
- Expected: exit code 0 (regression guard; no test or source file changed per TS-5)
- Coverage target: not applicable (no code change)

### Test Scenarios from SPEC.md

| ID | Scenario | Expected Result | Test Type |
|----|----------|-----------------|-----------|
| TS-1 | Search `doc/SPECIFICATION.md` for `agent_notify_visible_pane` (SPEC AC-1, AC-2) | Matches inside the Agent Status section in both the visible-pane bullet and the settings-key bullet | Doc check (text search) |
| TS-2 | Search `doc/AGENT-STATUS.md` for "not visible in the foreground window"; then search the Notifications section for `agent_notify_visible_pane` (SPEC AC-3) | First search: no match. Second search: at least one match | Doc check (text search) |
| TS-3 | Search the Notifications section for each of `agent_status_notifications`, `agent_notify_on_done`, `agent_notify_on_blocked`, `agent_notify_visible_pane` (SPEC AC-4) | Every key is found; the section states each listed setting and the global notification setting are default on, and still mentions the per-pane rate limit | Doc check (text search + reading) |
| TS-4 | Read the notification conditions written in both documents against `should_fire_agent_notification` (`src-tauri/src/notifications.rs:271-287`), `agent_status_pane_visible` (`src-tauri/src/app/agent_status.rs:45-57`), and the defaults in `src-tauri/src/settings/mod.rs` (SPEC AC-1, AC-3, AC-4) | Every stated condition and default matches the code; "visible" means the OS window is focused and the pane belongs to the active tab; both documents agree | Doc review (code cross-check) |
| TS-5 | `git diff --name-only <base revision>` (SPEC AC-5) | Every listed path is under `doc/`, `feature-docs/agent-notify-visible-pane-docs/`, or `test-docs/agent-notify-visible-pane-docs/` (the SPEC's declared workflow-record path); no path under `src-tauri/`, `crates/`, `scripts/`, or any other source or test location | Diff check |
| TS-6 | Read the lines added under `doc/` (`git diff -U0 <base revision> -- doc/`) and search them for "PR", "#29", "finding", "active-window", "previously", "no longer", "restores", "existing" (NFR3; planner-added scenario, SPEC.md has no scenario for NFR3) | Added lines state behavior only: no reason clauses, no PR / review-finding / feature references, no history wording | Doc review (diff reading) |

## Code Quality Verification

- Format: `cargo fmt --manifest-path src-tauri/Cargo.toml --check` and `bunx biome check .` — regression guard only; no formatted source changed
- Static analysis: none beyond the build commands above

## SPEC.md Compliance

### Success Criteria

| ID | Criterion | How to Verify |
|----|-----------|---------------|
| SC-1 | FR1 to FR4 are satisfied | TS-1, TS-2, TS-3, TS-4 |
| SC-2 | TS-1 to TS-5 all pass | Run TS-1 to TS-5 as listed above |
| SC-3 | NFR1 to NFR3 are satisfied | TS-5 (NFR1), TS-4 (NFR2), TS-6 (NFR3) |

### Functional Requirements Coverage

| Requirement | Tasks | Verification |
|-------------|-------|--------------|
| FR1 | task0001 | TS-1, TS-4 |
| FR2 | task0001 | TS-1 |
| FR3 | task0001 | TS-2, TS-4 |
| FR4 | task0001 | TS-3, TS-4 |
| NFR1 | task0001 | TS-5 |
| NFR2 | task0001 | TS-4 |
| NFR3 | task0001 | TS-6 |

## E2E Testing

Not applicable. The project has no E2E suite, and the change is documentation only.

## Manual Testing (E2E Not Possible)

None required from a human. TS-4 and TS-6 are reading checks the verify
phase performs directly against the repository. The design step was
skipped, so there is no mockup comparison.

## Performance / Security Verification (if applicable)

Not applicable (documentation only).

## Verification Summary

| Category | Items | Automated | E2E | Manual |
|----------|-------|-----------|-----|--------|
| Build (regression guard) | 2 | 2 | 0 | 0 |
| Test (regression guard) | 2 | 2 | 0 | 0 |
| Doc checks (text search / diff) | 4 (TS-1, TS-2, TS-3, TS-5) | 4 | 0 | 0 |
| Doc review (reading) | 2 (TS-4, TS-6) | 0 | 0 | 2 (performed by the verify phase) |
