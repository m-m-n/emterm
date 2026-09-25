# Verification Document: notification-capability-query-skip

## Overview

**Feature**: notification-capability-query-skip / **SPEC.md**: `feature-docs/notification-capability-query-skip/SPEC.md` / **IMPLEMENTATION.md**: `feature-docs/notification-capability-query-skip/IMPLEMENTATION.md`

Integrated verification of the conditional capability query in the Unix
notification worker. Task-level acceptance criteria live in `tasks/task0001.md`.
Run every command from the repository root (no `cd`).

## Build Verification

- Command (main): `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml`
- Command (cli_feature_gate): `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml --no-default-features`
- Expected: exit code 0, no errors, for both commands.

## Test Verification

- Command: `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib`
- Coverage target: no coverage tool is configured for this project. Coverage
  is judged by scenario: all four rows of the SPEC decision table (no
  metacharacter; metacharacter with an error, with a `body-markup` list, with
  a list omitting `body-markup`) must be exercised by TS-1 to TS-5 — minimum
  and target are both 100% of those rows.
- Known unrelated flakiness in the `--lib` suite: the `tabs.rs` replay tests
  under parallel execution and a rare `tmux_sockets` discover race. A failure
  confined to those tests is re-run before it is treated as a regression.

### Test Scenarios from SPEC.md

TS-1 to TS-6 correspond to SPEC TS1 to TS6. TS-7 to TS-10 are derived from
the SPEC requirements and Success Criteria (AC7, NFR1, NFR3 to NFR5) to give
every requirement a verifying scenario.

| ID | Scenario | Expected Result | Test Type |
|----|----------|-----------------|-----------|
| TS-1 | Titles and bodies without `&`, `<`, `>`: ASCII, non-ASCII, empty title or body, fallback title `emterm` (SPEC TS1) | Counting stub supplier records 0 calls; returned pair equals the input pair, and equals the existing `escape_for_send` result for each of the three query outcomes | Unit |
| TS-2 | Metacharacter in the title only (for example the OSC 9 fallback title `<tab title>`), under each of: query error, success containing `body-markup`, success omitting `body-markup` (SPEC TS2) | Stub records exactly 1 call; returned pair equals the existing `escape_for_send` result (both escaped for error and `body-markup`; unchanged for a list omitting it) | Unit |
| TS-3 | Metacharacter in the body only (a lone `&`, and a pre-existing `&amp;`), under the same three outcomes (SPEC TS3) | Stub records exactly 1 call; returned pair equals the existing `escape_for_send` result | Unit |
| TS-4 | Metacharacters in both fields, under the same three outcomes (SPEC TS4) | Stub records exactly 1 call; that single outcome decides both fields together | Unit |
| TS-5 | Quotes (`"`, `'`) and other non-metacharacter symbols in title and body (SPEC TS5) | Stub records 0 calls; returned pair equals the input | Unit |
| TS-6 | Full `--lib` suite and the `--no-default-features` check (SPEC TS6) | Both commands succeed; the existing `escape_for_send`, `body_markup_absence_confirmed`, `escape_body_markup` and notification-redaction tests pass with their assertions unchanged | Integration |
| TS-7 | Two consecutive gate calls with metacharacter input, each with its own stub and a different outcome | Each stub records exactly 1 call; each result follows its own outcome — nothing is carried over between calls | Unit |
| TS-8 | Identity property the short-circuit relies on: `escape_body_markup` over every printable ASCII character and a sample of non-ASCII characters | Every character outside `&`, `<`, `>` is unchanged; each of the three is changed; the metacharacter constant holds exactly those three | Unit |
| TS-9 | Doc comments in `src-tauri/src/callbacks.rs` (`notify_worker`, the D3 comment above the escape step, `NotifyRustSink`, `escape_for_send`) | Each describes the conditional query; a text search finds no comment claiming an unconditional query, "exactly once per send", "once per notification", or a synchronous D-Bus round-trip on the PTY-processing or UI thread | Static (review) |
| TS-10 | Structural review of the diff | `notify_worker` hands the capability query to the gate un-invoked and no other non-comment line invokes it; redaction still precedes the gate and reads the raw values; new items are Unix-only; no new cfg branch on enqueue, receive or dispatch; no caching primitive added; no `Cargo.toml` or `Cargo.lock` change | Static (review) |

## Code Quality Verification

- Format: not configured (`format_command` is empty in workflow.yaml). Do not
  run a crate-wide formatter; it rewrites unrelated files.
- Static analysis: not configured. The build commands above are the compile
  gate.

## SPEC.md Compliance

### Success Criteria

| ID | Criterion | How to Verify |
|----|-----------|---------------|
| AC1 | No metacharacter in title or body: 0 capability queries; dispatched pair equals input | TS-1, TS-5 |
| AC2 | Metacharacter in the title only: exactly 1 query; escaped on error or `body-markup`; unchanged when the list omits `body-markup` | TS-2 |
| AC3 | Metacharacter in the body only: same results as AC2 with the fields swapped | TS-3 |
| AC4 | Existing escape tests pass with unchanged expectations | TS-6 |
| AC5 | `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib` passes | TS-6 |
| AC6 | `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml --no-default-features` passes | TS-6 |
| AC7 | No doc comment in `src-tauri/src/callbacks.rs` states an unconditional or exactly-once-per-notification query | TS-9 |

### Functional Requirements Coverage

| Requirement | Tasks | Verification |
|-------------|-------|--------------|
| FR1 | task0001 | TS-1, TS-5 (unit); TS-10 (production wiring review); Manual D-Bus observation |
| FR2 | task0001 | TS-2, TS-3, TS-4 (unit) |
| FR3 | task0001 | TS-1, TS-2, TS-3, TS-4 (equality with the existing `escape_for_send`); TS-8 (identity property) |
| FR4 | task0001 | TS-1 to TS-5 (injected counting stub, no D-Bus) |
| FR5 | task0001 | TS-9 (static review) |
| NFR1 | task0001 | TS-7 (unit); TS-10 (no caching primitive) |
| NFR2 | task0001 | TS-6 (existing tests unchanged); TS-8 |
| NFR3 | task0001 | TS-6 (`--no-default-features` check); TS-10 (cfg review) |
| NFR4 | task0001 | TS-6 (existing redaction tests); TS-10 (redaction order review) |
| NFR5 | task0001 | TS-10 (no manifest or lockfile change) |
| NFR6 | task0001 | TS-6 (existing assertions unchanged, checked by diff) |

## E2E Testing

Not applicable: no E2E framework is detected for this project and the SPEC
lists no E2E run command.

## Manual Testing (E2E Not Possible)

- [ ] D-Bus observation on a Linux desktop session with a notification
      server: run a build of this branch, watch the session bus with
      `dbus-monitor --session "interface='org.freedesktop.Notifications'"`,
      then emit OSC 9 notifications with an explicit title from a shell
      inside eMterm. For plain text (for example
      `printf '\e]9;hello;world\a'`) only a `Notify` call appears, with no
      `GetCapabilities` call. For a body containing a metacharacter (for
      example `printf '\e]9;hello;a <b> c\a'`) exactly one `GetCapabilities`
      call precedes the `Notify` call. (FR1, FR2)
- [ ] In the same session, both notifications display, and the
      metacharacter one looks the same as it did before this feature. (FR3)
- [ ] Optional: the Windows cross-build (`make win-build`) compiles without
      new errors or warnings from `src-tauri/src/callbacks.rs`. (NFR3)

## Performance / Security Verification (if applicable)

- Performance (FR1): no capability-query D-Bus connection or round-trip for
  a notification whose title and body contain no metacharacter — TS-1 and
  TS-5 (unit), Manual D-Bus observation (real bus).
- Security (fail-closed, NFR1 and NFR2): with a metacharacter present, a
  query error or a list containing `body-markup` escapes both fields — TS-2
  to TS-4; no capability caching — TS-7 and TS-10.

## Verification Summary

| Category | Items | Automated | E2E | Manual |
|----------|-------|-----------|-----|--------|
| Build | 2 | 2 | 0 | 0 |
| Unit tests | 7 (TS-1 to TS-5, TS-7, TS-8) | 7 | 0 | 0 |
| Integration / regression | 1 (TS-6) | 1 | 0 | 0 |
| Static review | 2 (TS-9, TS-10) | 0 | 0 | 2 |
| Manual checks | 3 | 0 | 0 | 3 |
