# Implementation Plan: restored-pane-read-flake-fix

## Overview

Test-only fix: the read side of
`mux::session::pane::tests::from_restored_pane_can_write_and_read_through_its_adopted_master`
becomes a bounded read-until-marker check that reports the collected bytes on
failure, backed by deterministic regression tests, and the comment in
`src-tauri/src/mux/upgrade/tests.rs` that calls the read deterministic is
reworded to match. Requirements: `REQUIREMENTS.md`, `SPEC.md`.

## Technology Stack

- **Language**: Rust (test code only, inside existing `#[cfg(test)]` modules).
- **Key libraries**: the Rust standard library (threads, timing) and one
  channel with a receive timeout taken from what the crate already has (the
  standard library's channel or the existing `crossbeam-channel` dependency).
- **New dependencies**: none (NFR2). No license to record; `Cargo.toml` and
  `Cargo.lock` stay unchanged.

## Layer Structure

No production layer is touched (NFR1). Both tasks change only test modules:

| Area | File | Changed by |
|------|------|------------|
| Pane-level tests | `src-tauri/src/mux/session/pane/tests.rs` | task0001 |
| Upgrade-level tests | `src-tauri/src/mux/upgrade/tests.rs` | task0002 |

Production code paths (`MuxPane::from_restored`, `write_input`,
`pane/handles.rs`) and `src-tauri/src/pty/mod.rs` are read-only references.

## Shared Components

| Component | Responsibility | Contract (pre/postcondition) | Used by tasks |
|-----------|----------------|------------------------------|---------------|
| Read-back check of the target test (`from_restored_pane_can_write_and_read_through_its_adopted_master`) | Proves a restored pane's input write is readable back through its adopted master | Name unchanged; stays unix-gated. After writing `restored-write` plus a newline through the restored pane, it reads the PTY echo through a reader cloned from the master BEFORE the master is handed to the pane, on a test-owned reading thread. No pane reader thread competes for the echo. It accumulates chunks until the 14-byte marker `restored-write` is present or a 5-second deadline (absolute, measured from the start of the wait) passes. It fails, with the collected bytes in the message, on deadline, end of data or read error. It never claims that a single read is deterministic. | task0001 (implements it), task0002 (describes it in the upgrade test comment) |

## Conventions

- **Test-only change (NFR1)**: only test code and test comments under
  `src-tauri/src/` change. Neither task edits a production file, `Cargo.toml`
  or `Cargo.lock`.
- **Platform gating (NFR3)**: the target test keeps its unix gate. New test
  helpers use no unix-only API, so they need no gate. The
  `--no-default-features` check keeps compiling.
- **Test names**: the target test's name is unchanged (SPEC A1), so the
  `test-docs/` records of earlier features that list it need no update under
  `.claude/rules/test-docs-records.md`.
- **Wording**: no test comment or failure message in this feature describes
  the PTY echo read as deterministic or as a single read.
- **Predecessor record**:
  `feature-docs/restored-pane-read-flake-investigation/INVESTIGATION.md` stays
  as written (SPEC A6).

## Cross-task Design Decisions

### D1: Deadline enforced by a helper reading thread plus a channel receive timeout

- **Decision**: the bounded wait (FR2) uses a test-owned helper thread. The
  thread performs the blocking reads and hands each chunk, end of data, or a
  read error back through a channel. The waiting side receives with a timeout
  equal to the time left before the deadline. `poll` on the master fd is not
  used (SPEC A3 leaves this choice to planning).
- **Rationale**: the same read-until-marker routine accepts any reader, so
  the scripted-reader regression tests (FR4) exercise exactly the code path
  the real-PTY test uses. It needs no raw-fd or unsafe code, so it adds no
  unix-only surface (NFR3). It follows the accumulate-until-deadline pattern
  already used by `pty_round_trip_echo_hello` in `src-tauri/src/pty/mod.rs`.
- **Affected tasks**: task0001 implements it. task0002's comment must not
  claim that the pane-level test has no background reader thread; it may say
  that no pane reader thread competes for the echo.

### D2: Deadline value

- **Decision**: the target test waits at most 5 seconds for the marker
  (NFR4). A passing run returns as soon as the marker is complete.
- **Affected tasks**: task0001 (value), task0002 (the comment may refer to a
  bounded deadline without naming the value).

## Risk Assessment

| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|------------|
| The helper reading thread stays blocked in a read after the waiting side has returned | High (expected on the success path) | Low | The thread is not joined. It ends by itself when its reader reports end of data or an error (the PTY closes when the test's pair and pane are dropped) or when a send finds the receiver gone. It never panics in either case. Scripted readers in the regression tests are released or trickle data, so their threads end promptly. |
| Timing assertions in the regression tests flake under load | Medium | Medium | Timing checks only assert upper bounds that are generous compared with the deadline used. A wrong implementation fails them deterministically. No lower-bound or tight timing check is used. |
| A slow trickle of non-marker bytes extends the wait past the deadline | Low | Medium | The deadline is absolute, measured once at the start of the wait. Each wait uses only the remaining time. A regression test covers the trickle case. |
| Ungated helper code fails to compile on Windows | Low | Medium | Helpers use only platform-neutral std and channel facilities. The target test keeps its unix gate. |

## Open Questions

None.
