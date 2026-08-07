# Feature: plugin-stop-hook-done

## Overview

The eMterm plugin's Stop hook currently reports `state=idle`, but eMterm only
fires OS notifications for the Blocked and Done states
(`src-tauri/src/notifications.rs:226-228` `is_qualifying_agent_state`), so the
response-completion notification never fires. This feature changes the Stop
hook's argument from `idle` to `done` and updates the one test assertion that
binds Stop to `idle`. See `REQUIREMENTS.md` for the full requirements document.

## Objectives

- Make eMterm's OS notification fire when Claude Code finishes a response.
  Today the Stop hook in `plugins/emterm/hooks/hooks.json` sends `state=idle`
  while eMterm's firing set is Blocked / Done only
  (`src-tauri/src/notifications.rs:226-228` `is_qualifying_agent_state`), so
  the completion notification is structurally impossible.
- Align the plugin's Stop-time reported state with eMterm's own design (`done`
  is fully implemented, and `done` + read is aliased to `IDLE_BADGE_EMOJI` so
  it does not stick: `src-tauri/src/ui/tab_bar.rs:1855, 1872-1879`).

## User Stories

### US1: Receive an OS notification on response completion

As an eMterm user running Claude Code, I want an OS notification when the
response completes in an inactive tab or unfocused window, so that I notice
completion without watching the pane.

**Acceptance Criteria:**
- [ ] The Stop `args` in `plugins/emterm/hooks/hooks.json` is `["done"]`.
- [ ] Manual check: with an inactive tab (or unfocused window), an OS
      notification appears when Claude Code's response completes (verified by
      the user).

### US2: Keep the hook test suite consistent with the shipped hooks.json

As a maintainer, I want the hook test to assert the new Stop state, so that
`bun test` reflects the shipped `hooks.json`.

**Acceptance Criteria:**
- [ ] The assertion at `plugins/emterm/hooks/scripts/notify-status.test.ts:420`
      is updated to `["Stop", "done"]`.
- [ ] `bun test` passes.

## Technical Requirements

### Functional Requirements

- **FR1 — Stop hook sends done:** Change the `args` of the Stop entry in
  `plugins/emterm/hooks/hooks.json` from `["idle"]` to `["done"]` (currently
  lines 46-47 of `hooks.json`). Do not change the other events
  (`UserPromptSubmit` / `PostToolUse` / `PostToolUseFailure` = `working`,
  `PermissionRequest` / `Notification` = `blocked`).
- **FR2 — Test expectation update:** Update the `test.each` table row
  `["Stop", "idle"]` at
  `plugins/emterm/hooks/scripts/notify-status.test.ts:420` to
  `["Stop", "done"]`. This is the only place in that file binding Stop to
  `idle` (line 175's `ALLOWED_STATES` and line 486's `idle_prompt` are
  unrelated and need no change).

### Non-Functional Requirements

- **NFR1 — No script change:** `notify-status.sh` needs no change: `done` is
  already in the state-argument whitelist (`idle|working|blocked|done`,
  `notify-status.sh:18-24`).
- **NFR2 — Core untouched:** Do not change the eMterm core (`src-tauri/`) —
  out of scope by directive, and the `done` side is already implemented.
- **NFR3 — Edit the repository source only:** Edits are limited to the
  in-repository `plugins/emterm/` sources. Do not directly edit the copy under
  `~/.claude/plugins/cache/` (the marketplace points at this repository as a
  directory source).
- **NFR4 — Preserve the hook command format:** Keep the existing `hooks.json`
  command format — the `${CLAUDE_PLUGIN_ROOT}` prefix and `timeout 3`
  (`notify-status.test.ts:424-450` validates the format).
- **NFR5 — Notification path untouched:** Do not change the notification path
  (`terminalSequence`'s OSC 777, which reaches the local eMterm even over SSH,
  and D-Bus notify-rust).

## Implementation Approach

### Architecture

**Data flow (unchanged by this feature except for the state value):**

```
Claude Code (Stop event)
  → hooks.json Stop entry: notify-status.sh <state>     ← FR1 changes <state>
  → terminalSequence OSC 777 (reaches local eMterm over SSH)
  → eMterm notifications.rs is_qualifying_agent_state (Blocked / Done only)
  → D-Bus notify-rust → OS notification
```

The only behavioural edit is the `<state>` value carried by the Stop entry;
every stage downstream of it is already implemented and is left unchanged
(NFR1, NFR2, NFR5).

### Dependencies

**Internal Dependencies:**
- `plugins/emterm/hooks/scripts/notify-status.sh`: consumes the state argument;
  its whitelist already accepts `done` (NFR1) — unchanged.
- `src-tauri/src/notifications.rs`: `is_qualifying_agent_state` (226-228)
  restricts firing to Blocked / Done; `event_type_notifications_enabled`
  (239-249) gates on `notification_enabled` / `agent_status_notifications` /
  `agent_notify_on_done`; `AGENT_NOTIFICATION_RATE_LIMIT` (221) is the 30 s
  rate limit — all unchanged (NFR2).
- `src-tauri/src/ui/tab_bar.rs`: `done` + read aliases to `IDLE_BADGE_EMOJI`
  (1855, 1872-1879) — unchanged (NFR2).

**External Dependencies:**
- D-Bus notify-rust — the OS notification transport, unchanged (NFR5).

### File Structure

```
plugins/emterm/hooks/
├── hooks.json                       # FR1: Stop args ["idle"] → ["done"] (lines 46-47)
└── scripts/
    ├── notify-status.sh             # unchanged (NFR1)
    └── notify-status.test.ts        # FR2: line 420 ["Stop","idle"] → ["Stop","done"]
```

## Test Scenarios

### Unit Tests

- [ ] **TS1** (FR1, FR2): `bun test` — the table-driven test in
      `notify-status.test.ts` (lines 416-436) is consistent with the updated
      `hooks.json`. The test reads the real `hooks.json` via `readHooksJson()`,
      so FR1 and FR2 must be changed together or it fails.

### Integration Tests

Covered by TS1: the test reads the shipped `hooks.json` file rather than a
fixture, so it exercises the hook definition and the expectation together.

### E2E Tests

**Existing E2E tests**: None
**Run command**: Not detected

### Manual Verification

- [ ] **TS2** (FR1): With the pane hidden, complete a response → exactly one OS
      notification. Within the 30 s rate limit
      (`AGENT_NOTIFICATION_RATE_LIMIT`, `notifications.rs:221`), a second
      consecutive completion producing no notification is by design.

### Edge Cases

- [ ] Stop can fire for reasons other than response completion (e.g. a stop
      after user interruption). Because `done` + read is aliased to
      `IDLE_BADGE_EMOJI` (`tab_bar.rs:1855`), `done` does not stick, and the
      task description explicitly specifies Stop→`done`, so this is accepted.
- [ ] `SubagentStop` / `StopFailure` do not exist in the hooks (the test at
      lines 404-405 asserts `undefined`) and are unaffected by this change.

## Assumptions

- The runtime's per-event toggle `agent_notify_on_done` is enabled. The gating
  (`notifications.rs:239-249` `event_type_notifications_enabled`) requires this
  toggle in addition to `notification_enabled` / `agent_status_notifications`,
  but the task description stated only the latter two as confirmed.
- Manual verification is performed with the pane hidden and outside the rate
  limit (agent-status notifications fire only while the pane is hidden).

## Success Criteria

- [ ] The Stop `args` in `plugins/emterm/hooks/hooks.json` is `["done"]`.
- [ ] The assertion at `plugins/emterm/hooks/scripts/notify-status.test.ts:420`
      is updated to `["Stop", "done"]`.
- [ ] `bun test` passes.
- [ ] Manual check: with an inactive tab (or unfocused window), an OS
      notification appears when Claude Code's response completes (verified by
      the user).

## Open Questions

> **Note**: 未解決の要件は workflow.yaml で `status: tbd` として管理されています。
> plan フェーズの実行前に解決してください。

None — every requirement is `resolved`.

## Design Step

Skipped. The change is one JSON configuration value and one test expectation,
with no change to any UI surface, visual element or layout; the existing
notification / badge UI is already implemented and untouched.

## References

- Requirements document: `feature-docs/plugin-stop-hook-done/REQUIREMENTS.md`
- `plugins/emterm/hooks/hooks.json`: Stop entry (lines 46-47)
- `plugins/emterm/hooks/scripts/notify-status.test.ts`: expectation table
  (416-436, target line 420), format validation (424-450), undefined-event
  validation (404-405)
- `plugins/emterm/hooks/scripts/notify-status.sh`: state whitelist (18-24)
- `src-tauri/src/notifications.rs`: `is_qualifying_agent_state` (226-228),
  `AGENT_NOTIFICATION_RATE_LIMIT` (221),
  `event_type_notifications_enabled` (239-249)
- `src-tauri/src/ui/tab_bar.rs`: `IDLE_BADGE_EMOJI` alias (1855, 1872-1879)
