# Verification Document: plugin-stop-hook-done

## Overview

**Feature**: plugin-stop-hook-done /
**SPEC.md**: `feature-docs/plugin-stop-hook-done/SPEC.md` /
**IMPLEMENTATION.md**: `feature-docs/plugin-stop-hook-done/IMPLEMENTATION.md`

## Build Verification

- Command: `bun run typecheck`
- Expected: exit code 0, no errors

## Test Verification

- Command: `bun test`
- Coverage target: no numeric target — the feature adds no new logic (one
  configuration value + one test expectation). The full existing suite must
  pass with exit code 0.

### Test Scenarios from SPEC.md

| ID | Scenario | Expected Result | Test Type |
|----|----------|-----------------|-----------|
| TS1 | `bun test` — the table-driven suite in `notify-status.test.ts` (lines 416-436) against the updated `hooks.json`. The suite reads the shipped `hooks.json` via `readHooksJson`, so it exercises the hook definition and the expectation together; FR1 and FR2 diverging in either direction fails it. The format-validation tests (lines 424-450) additionally confirm the `${CLAUDE_PLUGIN_ROOT}` prefix and `timeout 3` command form (NFR4). | Suite passes, exit code 0 | Integration (existing suite) |
| TS2 | With the pane hidden (inactive tab or unfocused window), a Claude Code response completes → exactly one OS notification fires. | One notification; a second consecutive completion within the 30 s rate limit (`AGENT_NOTIFICATION_RATE_LIMIT`) producing no notification is by design | Manual |
| TS3 | Change-scope check: the feature's change set contains only `plugins/emterm/hooks/hooks.json` and `plugins/emterm/hooks/scripts/notify-status.test.ts`. No change under `src-tauri/` (NFR2), no change to `notify-status.sh` (NFR1), no edit outside the repository sources (NFR3). | Diff limited to the two files | Manual (diff inspection / review) |

## Code Quality Verification

- Format / static analysis: `bunx biome check .` — exit code 0

## SPEC.md Compliance

### Success Criteria

| ID | Criterion | How to Verify |
|----|-----------|---------------|
| SC1 | The Stop `args` in `plugins/emterm/hooks/hooks.json` is `["done"]` | TS1 (suite reads the real `hooks.json`) + file inspection |
| SC2 | The `notify-status.test.ts` expectation (line 420) binds Stop to `done` | TS1 + file inspection |
| SC3 | `bun test` passes | TS1 |
| SC4 | With an inactive tab (or unfocused window), an OS notification appears when the response completes | TS2 (user manual check) |

### Functional Requirements Coverage

| Requirement | Tasks | Verification |
|-------------|-------|--------------|
| FR1 | task0001 | TS1 (automated), TS2 (manual, end-to-end effect) |
| FR2 | task0001 | TS1 (automated) |
| NFR1 | task0001 | TS1 (whitelist already accepts `done`; script unchanged), TS3 (no diff on `notify-status.sh`) |
| NFR2 | task0001 | TS3 (no diff under `src-tauri/`) |
| NFR3 | task0001 | TS3 (diff limited to in-repository `plugins/emterm/` sources) |
| NFR4 | task0001 | TS1 (format-validation tests, lines 424-450) |
| NFR5 | task0001 | TS2 (notification arrives through the unchanged path), TS3 (no path-related code in the diff) |

## E2E Testing

No E2E test infrastructure exists in this project (`e2e_test_command` is
empty). Omitted.

## Manual Testing (E2E Not Possible)

Preconditions for TS2 (from SPEC.md Assumptions):

- The pane is hidden (agent-status notifications fire only while the pane is
  hidden).
- The runtime toggles `notification_enabled`, `agent_status_notifications`,
  and `agent_notify_on_done` are all enabled (the last one is an assumption —
  confirm it in the settings before judging a missing notification a failure).
- The check is performed outside the 30 s rate limit
  (`AGENT_NOTIFICATION_RATE_LIMIT`).

- [ ] TS2: complete a Claude Code response with the pane hidden → exactly one
      OS notification. A second consecutive completion within 30 s producing
      no notification is by design, not a failure.
- [ ] TS3: inspect the integrated diff — only the two in-scope files changed.

## Verification Summary

| Category | Items | Automated | E2E | Manual |
|----------|-------|-----------|-----|--------|
| Build (`bun run typecheck`) | 1 | 1 | 0 | 0 |
| Tests (`bun test` / TS1) | 1 | 1 | 0 | 0 |
| Code quality (`bunx biome check .`) | 1 | 1 | 0 | 0 |
| Manual scenarios (TS2, TS3) | 2 | 0 | 0 | 2 |
| **Total** | **5** | **3** | **0** | **2** |
