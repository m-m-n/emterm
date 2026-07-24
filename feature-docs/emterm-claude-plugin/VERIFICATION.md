# Verification Document: emterm-claude-plugin

## Overview

**Feature**: emterm-claude-plugin
**SPEC.md**: `feature-docs/emterm-claude-plugin/SPEC.md`
**IMPLEMENTATION.md**: `feature-docs/emterm-claude-plugin/IMPLEMENTATION.md`

## Build Verification

- Command: `bun run typecheck`
- Expected: exit code 0, no type errors.

## Test Verification

- Command: `bun test`
- Coverage target: 100% of `notify-status.ts` branches (allow-list rejection, missing emterm, tty-open failure, child non-zero, happy path). No numeric coverage threshold enforced by tooling; branch coverage checked by presence of the AC-3 through AC-7 tests from task0002.

### Test Scenarios from SPEC.md

| ID | Scenario | Expected Result | Test Type |
|----|----------|-----------------|-----------|
| TS-1 | `notify-status.ts` invoked with invalid state argument | Exit 0, no `emterm` spawn | Unit |
| TS-2 | `notify-status.ts` invoked when `emterm` is not on PATH | Exit 0, no `/dev/tty` open | Unit |
| TS-3 | `/dev/tty` open fails | Exit 0, no unhandled rejection | Unit |
| TS-4 | `emterm` child exits non-zero | Exit 0 | Unit |
| TS-5 | Happy path: fake `emterm` stdout is written verbatim to the tty sink | Sink received exact bytes, exit 0 | Unit |
| TS-6 | `marketplace.json`, `plugin.json`, `hooks.json` static validity | All parse; marketplace ↔ plugin name+version match; hooks use `${CLAUDE_PLUGIN_ROOT}` and no absolute paths / `..` | Integration (static) |
| TS-7 | All seven SKILL.md files present with correct `name` and non-empty English `description` | Static walk passes | Integration (static) |

## Code Quality Verification

- Format: no formatter configured for the plugin's TypeScript; skip.
- Static analysis: `bun run typecheck` (see Build Verification) is the type-level static analysis.

## SPEC.md Compliance

### Success Criteria

| ID | Criterion | How to Verify |
|----|-----------|---------------|
| SC-1 | All functional requirements FR1-FR8 implemented | Requirements coverage table below; task acceptance criteria |
| SC-2 | `bun test` and `bun run typecheck` pass | Build + Test Verification above |
| SC-3 | Local POC demonstrates state changes reaching an eMterm tab | Manual scenario M-1 below |
| SC-4 | README documents install path, prerequisites, known limitations | Task0004 acceptance criteria + spec review |
| SC-5 | No changes to `src-tauri/` | `git diff --name-only base_commit..parent_branch` shows nothing under `src-tauri/` |

### Functional Requirements Coverage

| Requirement | Tasks | Verification |
|-------------|-------|--------------|
| FR1 | task0001 | TS-6 |
| FR2 | task0001 | TS-6 |
| FR3 | task0002 | TS-6 + task0002 AC-1 |
| FR4 | task0002 | TS-1 through TS-5 |
| FR5 | task0003 | TS-7 |
| FR6 | task0003 | TS-7 |
| FR7 | task0004 | Manual read of README against AC-1..AC-7 |
| FR8 | (verify phase POC) | Manual scenario M-1 |
| NFR1 | task0002 (structure), verify (measurement) | Manual scenario M-1 records Bun startup time |
| NFR2 | task0002 | TS-1 (allow-list) + code review of spawn call form |
| NFR3 | task0001, task0002, task0003, task0004 | TS-6 (hooks path check); manual read for SKILL.md and README |
| NFR4 | task0004 | Manual read of README; verify no committed binaries via `git ls-files plugins/emterm/` inspection |

## E2E Testing

No project E2E framework detected. E2E coverage is provided by the manual scenarios below.

## Manual Testing (E2E Not Possible)

- [ ] M-1: **Local POC (fulfills FR8 and NFR1 measurement).**
  Preconditions:
  - eMterm release build available; user launches an eMterm tab.
  - `emterm` and `bun` on PATH in that tab.
  - Claude Code installed with local plugin dev support.

  Steps:
  1. From the eMterm tab, install the plugin locally against this branch (e.g. `claude --plugin-dir plugins/emterm ...` or the current-equivalent invocation for local plugin loading).
  2. Send a prompt in Claude Code; observe the eMterm tab state transitions to `working`, then to `idle` when Claude finishes responding.
  3. Trigger a Claude Code Notification event (e.g. a permission prompt or a long-running tool call the user must confirm); observe the tab state transitions to `blocked`.
  4. Measure Bun cold-start + full hook execution time using `time bun ${CLAUDE_PLUGIN_ROOT}/hooks/scripts/notify-status.ts idle` from a fresh shell; record wall-clock.
  5. Record results (observations + timing + Claude Code / eMterm versions used) in `feature-docs/emterm-claude-plugin/POC-RESULTS.md`.

  Expected result: state transitions visible in the eMterm tab; hook wall-clock comfortably under 3 s; results file committed.

- [ ] M-2: **README review.** Read `plugins/emterm/README.md` end-to-end; confirm every FR7 bullet from task0004 AC-1..AC-7 is present.

## Performance / Security Verification

- **NFR1 (performance)**: measured in M-1. Recorded threshold: total hook execution ≤ 2.5 s (leaves 500 ms head-room under the 3 s hook timeout). If exceeded on the reference machine, note as a follow-up in POC-RESULTS.md but do not block release for v0.1.0.
- **NFR2 (security)**: code review confirms `notify-status.ts` uses argv-array spawn (no shell string) and validates state via a hard-coded allow-list before any use. Static grep in the file for `sh -c` / `exec(` / template-string shell forms must return zero hits.

## Verification Summary

| Category | Items | Automated | E2E | Manual |
|----------|-------|-----------|-----|--------|
| Build | 1 | 1 | 0 | 0 |
| Test scenarios | 7 (TS-1..TS-7) | 7 | 0 | 0 |
| Success criteria | 5 (SC-1..SC-5) | 4 | 0 | 1 |
| Manual scenarios | 2 (M-1, M-2) | 0 | 0 | 2 |
