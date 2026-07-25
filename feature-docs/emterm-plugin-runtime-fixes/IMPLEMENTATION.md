# Implementation Plan: emterm-plugin-runtime-fixes

## Overview

Replace the plugin's dead `/dev/tty` hook transport with `terminalSequence` JSON output, rewrite the hook in POSIX sh with no subprocess, and close the remaining High/Medium findings. All changes live under `plugins/emterm/`.

## Technology Stack

- **POSIX sh** — the hook script. No interpreter beyond `/bin/sh`; no `bun`, `emterm`, or `python3` at runtime.
- **Bun** — development-time only, as the repository's existing test runner. Tests invoke the shell script as a subprocess.

No new dependencies. License compatibility with `project.license: MIT` is trivial — nothing is added.

## Layer Structure

Two independent areas, no shared code between them:

- `plugins/emterm/hooks/` — the transport rewrite (script + manifest).
- `plugins/emterm/skills/` and `plugins/emterm/README.md` — documentation-only changes.

Nothing outside `plugins/emterm/` is modified. `src-tauri/`, `crates/`, and `.claude-plugin/` stay untouched (FR9 only requires that the existing `version` values are *not* changed).

## Shared Components

| Component | Responsibility | Contract (pre/postcondition) | Used by tasks |
|-----------|----------------|------------------------------|---------------|
| Canonical agent-status sequence | The exact byte string the hook emits per state | Pre: state ∈ {idle, working, blocked, done}. Post: `ESC ] 777 ; emterm ; agent-status ; v=1 ; state=<state> ; name=claude-code ESC \` — byte-identical to `crate::agent_status::build` with name `claude-code`. Pinned verbatim in SPEC.md FR3. | task0001 (emits it), task0002 (asserts it in the manifest-level tests only if needed) |

The sequence is the one cross-task fact. It is fully specified in SPEC.md FR3, so each task implements against the spec rather than against the other task's output.

## Conventions

- **Silent rejection**: any invalid input path exits 0 with empty stdout AND empty stderr. No diagnostics, ever — the hook fires on every prompt.
- **POSIX only**: no `[[`, no arrays, no `local`, no `echo -e`, no `$'...'`. `case` for matching, `[` for tests, `printf` for output (never `echo` for content containing backslashes).
- **No shell evaluation of input**: no `eval`, no backticks, no `$(...)` applied to arguments. Only the validated literal state is interpolated.
- **Skill hardening wording**: the four display skills adopt the same structure `mux-send/SKILL.md` already uses — a required-safety section naming argv-based invocation, plus adversarial examples. Read that file for the established shape rather than inventing a new one.
- **Exec form**: `hooks.json` entries carry `command` (the script path only) and `args` (the state), never a single joined string.

## Cross-task Design Decisions

### D1: Hook builds the sequence itself; `emterm` is never invoked

The hook constructs the OSC 777 string directly instead of spawning `emterm agent-status`. This is what makes POSIX sh viable and simultaneously removes: the tmux DCS-wrapping conflict (the CLI wraps for tmux, and DCS is outside the `terminalSequence` allowlist), per-prompt process-chain latency, the SIGKILL-escalation gap, and the `bun` prerequisite.

Cost: the wire format now exists in two places (`src-tauri/src/agent_status.rs` and the hook). Accepted — the format is one stable line, and SPEC.md FR3 pins the exact bytes with tests asserting the literal, so Rust-side drift surfaces as a test failure.

### D2: No internal timeout

The previous implementation carried a 2-second internal deadline racing Claude Code's 3-second hook timeout. With no subprocess and no device open there is nothing to wait on, so the mechanism is removed entirely rather than retained "just in case". Adding a timer back would be dead code.

### D3: Tests invoke the script as a subprocess

Test files stay TypeScript under `bun test` (the repository's runner), but they execute `notify-status.sh` as a child process and assert stdout plus exit code. This keeps the shipped artifact dependency-free while reusing the existing harness. Tests are the only place Bun touches this feature.

### D4: `notify-status.ts` is deleted, not deprecated

The old script is removed in the same task that adds the new one, so no dead path can be invoked by a stale manifest. The manifest change (exec form + new filename) and the script replacement therefore belong to the same task.

### D5: Documentation changes are a separate task from the transport

The README and the four display SKILL.md files are prose with no dependency on the hook implementation. Splitting them out keeps each task to one reviewable concern and lets both run in parallel.

## Risk Assessment

| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|------------|
| Shell portability slip (a bashism reaching the script) | Medium | Medium (fails on dash/ash) | Explicit convention list above; a test asserts the script runs correctly under `sh` specifically, not the developer's login shell. |
| Wire format drifts from the Rust canonical builder | Low | High (silent no-op in eMterm) | SPEC.md FR3 pins the bytes; TS-9 asserts the literal against the documented canonical form. |
| Notification matcher regex matches a longer type as a substring | Medium | Medium (blocked fires on the wrong events) | FR5 requires the matcher to be anchored; TS-7 tests both the matching and the non-matching type sets. |
| JSON escaping of the two ESC bytes and the trailing backslash is wrong | Medium | High (field ignored, silent failure) | TS-1 parses the output as JSON and compares the decoded value to the canonical sequence, so an escaping bug fails the test rather than shipping. |

## Open Questions

None. The transport assumption was verified empirically before the spec was frozen (SPEC.md "Verified Assumptions").
