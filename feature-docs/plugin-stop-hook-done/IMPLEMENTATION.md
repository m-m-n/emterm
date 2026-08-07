# Implementation Plan: plugin-stop-hook-done

## Overview

Change the eMterm plugin's Stop hook reported state from `idle` to `done` so
that eMterm's OS notification fires when a Claude Code response completes, and
update the single test expectation that binds Stop to `idle`. One configuration
value and one test-table value change; everything downstream (script whitelist,
OSC 777 path, eMterm core) is already implemented and stays untouched.

## Technology Stack

- **JSON (Claude Code plugin hook definition)**: `plugins/emterm/hooks/hooks.json` — carries the state argument per hook event.
- **TypeScript / Bun test**: `plugins/emterm/hooks/scripts/notify-status.test.ts` — table-driven suite that validates the shipped `hooks.json`.
- **New dependencies**: none. License impact: none (`project.license: MIT` unchanged; nothing to record beyond this line).

## Layer Structure

Unchanged. No new component, module, or layer is introduced; the change stays
inside the existing plugin hook definition and its test suite.

## Shared Components

None — this feature is a single task, so there is no cross-task contract.

| Component | Responsibility | Contract (pre/postcondition) | Used by tasks |
|-----------|----------------|------------------------------|---------------|
| (none)    | —              | —                            | —             |

## Conventions

- **Edit repository sources only** (NFR3): all edits happen under the
  in-repository `plugins/emterm/` tree; the copy under `~/.claude/plugins/cache/`
  is never edited directly (the marketplace points at this repository as a
  directory source).
- **Preserve the hook command format** (NFR4): the `hooks.json` command keeps
  its existing `${CLAUDE_PLUGIN_ROOT}` prefix and `timeout 3` form — the
  existing format-validation tests enforce this.
- **Do not touch** (NFR1 / NFR2 / NFR5): `notify-status.sh` (its state
  whitelist already accepts `done`), the eMterm core (`src-tauri/`), and the
  notification path (OSC 777 via terminal sequence, D-Bus notify-rust).

## Cross-task Design Decisions

### D1: FR1 and FR2 land in one atomic task

The test suite reads the shipped `hooks.json` file itself (via its
`readHooksJson` helper) rather than a fixture, so the hook-definition edit
(FR1) and the expectation edit (FR2) are coupled: landing either one alone
leaves `bun test` red on an intermediate commit. Both edits are therefore a
single task (task0001), and the feature is not decomposed further.

Affected tasks: task0001 (the only task).

## Risk Assessment

| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|------------|
| Stop fires for reasons other than response completion (e.g. stop after user interruption), briefly showing `done` | Medium | Low | Accepted by design: `done` + read is aliased to the idle badge in eMterm core, so `done` does not stick; the task description explicitly specifies Stop→`done` |
| Runtime per-event toggle `agent_notify_on_done` disabled in the verification environment | Low | Low (manual check TS2 would not fire) | Listed as an explicit precondition of the manual verification in VERIFICATION.md |
| Only one of the two files edited (partial change) | Low | Medium (`bun test` fails) | D1: both edits are in one task; TS1 catches any divergence because the test reads the real `hooks.json` |

## Open Questions

- [ ] The runtime per-event toggle `agent_notify_on_done` is assumed enabled
      (SPEC.md Assumptions — the task description confirmed only
      `notification_enabled` / `agent_status_notifications`). This is a
      precondition of manual scenario TS2, not a blocker for implementation.
