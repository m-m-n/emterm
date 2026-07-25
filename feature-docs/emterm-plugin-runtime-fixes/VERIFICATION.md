# Verification Document: emterm-plugin-runtime-fixes

## Overview

**Feature**: emterm-plugin-runtime-fixes
**SPEC.md**: `feature-docs/emterm-plugin-runtime-fixes/SPEC.md`
**IMPLEMENTATION.md**: `feature-docs/emterm-plugin-runtime-fixes/IMPLEMENTATION.md`

## Build Verification

- Command: `bun run typecheck`
- Expected: exit code 0, no type errors.

## Test Verification

- Command: `bun test`
- Coverage target: every branch of `notify-status.sh` (valid state, wrong arg count, non-allow-listed state, metacharacter argument) plus the static manifest and skill assertions. No numeric threshold is enforced by tooling; coverage is checked by the presence of TS-1 through TS-11.

### Test Scenarios from SPEC.md

| ID | Scenario | Expected Result | Test Type |
|----|----------|-----------------|-----------|
| TS-1 | Each valid state produces JSON with the single key `terminalSequence`, decoding to the FR3 canonical sequence | Parsed and decoded value matches the literal for that state | Unit |
| TS-2 | Argument outside the allow-list (`""`, `"invalid"`, `"WORKING"`) | Empty stdout, empty stderr, exit 0 | Unit |
| TS-3 | Zero positional arguments | Empty stdout, empty stderr, exit 0 | Unit |
| TS-4 | Two or more positional arguments (`working extra`) | Empty stdout, empty stderr, exit 0 | Unit |
| TS-5 | Argument with shell metacharacters (`"working; touch PWNED"`, `"$(id)"`) | Empty stdout, exit 0, no file created | Unit |
| TS-6 | `hooks.json` structural check | Valid JSON; all five hooks exec form; `${CLAUDE_PLUGIN_ROOT}`-prefixed commands; no absolute path, no `..`; `timeout` 3; no `SubagentStop` | Integration |
| TS-7 | `Notification` matcher behaviour | Matches `permission_prompt` / `elicitation_dialog` / `agent_needs_input`; does not match `idle_prompt` / `auth_success` / `elicitation_complete` / `elicitation_response` | Integration |
| TS-8 | Legacy artifacts removed | `notify-status.ts` absent; `notify-status.sh` contains no `/dev/tty`, `bun`, `eval`, backticks, or command substitution (`$(`), and no invocation of the `emterm` binary — the sole occurrence of the string `emterm` is the FR3 payload literal `emterm;agent-status;` | Integration |
| TS-9 | Wire-format fidelity | Emitted sequence per state is byte-identical to the canonical form documented in SPEC.md FR3 and produced by `src-tauri/src/agent_status.rs` with name `claude-code` | Integration |
| TS-10 | Skill static scan | All seven SKILL.md pass the existing checks; the four display skills additionally document the Bash-first quoted-and-`--`-delimited invocation form (with the `'\''` escaping rule), the resolve-`~`-before-quoting rule stated adjacent to the splice rule as one invariant (not an exception), and the double-quote-is-not-a-substitute warning with a quote-type-discriminating adversarial example; `mux-send/SKILL.md` additionally documents the file-redirection primary form with the staging file required via the Write tool (Bash-based creation forbidden for untrusted text, no heredoc anywhere), the "nothing enters the command line" claim qualified to a shell-free write, the redirect-target requirement (model-chosen, absolute, temp directory, no `~`), the `--text` single-quote rule shown as a complete quoted value, pane-ID validation, adversarial examples describing the destination pane's actual execution behaviour, and a consent-conditional closing instruction | Integration |
| TS-11 | Version pinned | `marketplace.json` and `plugin.json` both report `0.1.0` | Integration |

## Code Quality Verification

- Format: no formatter is configured for shell or the plugin's TypeScript; skipped.
- Static analysis: `bun run typecheck` covers the test files. The shell script's portability is covered behaviourally by running it under `sh` in TS-1 through TS-5.

## SPEC.md Compliance

### Success Criteria

| ID | Criterion | How to Verify |
|----|-----------|---------------|
| SC-1 | FR1-FR10 implemented | Requirements coverage table below plus task acceptance criteria |
| SC-2 | `bun test` and `bun run typecheck` pass | Build + Test Verification above |
| SC-3 | State transitions, including `blocked`-to-`working` recovery and the `StopFailure` path, observed on a real eMterm tab | Manual scenario M-1 |
| SC-4 | `notify-status.ts` gone; nothing references `bun` or `/dev/tty` | TS-8, plus `git grep -n 'dev/tty\|bun' plugins/emterm/` returning only test-harness references |
| SC-5 | Version remains `0.1.0` | TS-11 |
| SC-6 | No changes outside `plugins/` and `feature-docs/` | `git diff --name-only <base_commit>..<parent_branch>` shows no path under `src-tauri/`, `crates/`, or `.claude-plugin/` |

### Functional Requirements Coverage

| Requirement | Tasks | Verification |
|-------------|-------|--------------|
| FR1 | task0001 | TS-1, TS-8 |
| FR2 | task0001 | TS-2, TS-3, TS-4, TS-5 |
| FR3 | task0001 | TS-1, TS-9 |
| FR4 | task0001 | TS-6 |
| FR5 | task0001 | TS-7 |
| FR6 | task0001 | TS-8 |
| FR7 | task0002 | TS-10 |
| FR8 | task0002 | M-2 |
| FR9 | task0002 | TS-11 |
| FR10 | task0007 | TS-10 |
| NFR1 | task0001 | TS-8 (no subprocess/timer references in the script) |
| NFR2 | task0001 | TS-2, TS-5 |
| NFR3 | task0001 | TS-1 through TS-5, executed via `sh` |
| NFR4 | task0001 | TS-2, TS-3, TS-4 (empty stderr asserted) |

## E2E Testing

No project E2E framework. E2E coverage is the manual scenarios below.

## Manual Testing (E2E Not Possible)

- [ ] M-1: **Live transition check (fulfils SC-3).**

  Preconditions: the plugin installed locally from this branch; Claude Code v2.1.141 or later; an eMterm tab running Claude Code. The `emterm` binary is NOT required for this scenario.

  Steps:
  1. Send a prompt. Observe the eMterm tab badge become `working` (filled, primary colour).
  2. Wait for the response to complete. Observe the badge become `idle` (filled, `on_surface_variant`).
  3. Trigger a permission prompt (any tool call requiring approval). Observe the badge become `blocked` (`on_error_container`).
  4. Approve it. Observe the badge return to `working` (filled, primary colour) as soon as the approved tool call completes — this is the `PostToolUse` hook firing — then let the response finish and confirm the badge settles on `idle` and does NOT flip back to `blocked` from the ordinary idle notification.
  5. Force an API error (e.g. a rate limit or an auth failure) so the turn ends via `StopFailure` instead of `Stop`. Observe whether the badge settles on `idle`. `StopFailure` output and exit codes are documented as ignored by Claude Code, so treat this step as informational: record what was observed (including "no change" or "inconclusive") rather than treating any outcome as a hard pass/fail.

  Expected: all transitions in steps 1-4 visible, with step 4 showing the `blocked` -> `working` recovery and no spurious `blocked` afterward; step 5's result recorded either way.

  Note: badge shape distinguishes states — `Working`/`Idle` always render filled, `Blocked`/`Done` render as a ring once seen (`src-tauri/src/ui/tab_bar.rs`, `agent_badge_filled`). This is the same discriminator used in the pre-spec POC.

- [ ] M-2: **README review (fulfils FR8).** Read `plugins/emterm/README.md` end to end and confirm each of task0002's AC-1 through AC-5 holds.

## Performance / Security Verification

- **NFR1 (performance)**: verified structurally rather than by measurement — TS-8 confirms the script contains no process spawn, no device open, and no timer. With one `printf` as the only work, no timing threshold is meaningful.
- **NFR2 (security)**: TS-5 confirms metacharacter arguments are rejected with no output and no side effect. A static grep of `notify-status.sh` for `eval`, backticks, and `$(` over argument values must return zero hits (TS-8).
- **Known gap — no ordering coverage:** no automated test covers the ordering between `PostToolUse` and an open permission-prompt `Notification` (see SPEC.md Edge Cases, finding `cm-posttooluse-overwrites-blocked`). The correct fix is a precedence rule inside eMterm's own agent-status state machine (`src-tauri/`), which is out of scope for this feature; this feature only records the gap.

## Verification Summary

| Category | Items | Automated | E2E | Manual |
|----------|-------|-----------|-----|--------|
| Build | 1 | 1 | 0 | 0 |
| Test scenarios | 11 (TS-1..TS-11) | 11 | 0 | 0 |
| Success criteria | 6 (SC-1..SC-6) | 5 | 0 | 1 |
| Manual scenarios | 2 (M-1, M-2) | 0 | 0 | 2 |
