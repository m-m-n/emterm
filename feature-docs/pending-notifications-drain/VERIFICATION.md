# Verification Document: pending-notifications-drain

## Overview

**Feature**: pending-notifications-drain
**SPEC.md**: `feature-docs/pending-notifications-drain/SPEC.md`
**IMPLEMENTATION.md**: `feature-docs/pending-notifications-drain/IMPLEMENTATION.md`

This document describes the INTEGRATED verification of the whole feature.
Task-level acceptance criteria live in `tasks/task0001.md`.

All commands below are run from the project root
(`/home/sakura/src/my_projects/tauri/emterm`). Never change directory into the
crate — the manifest path and the target directory are always passed
explicitly.

## Build Verification

- Command (default features):
  ```
  CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml
  ```
- Command (CLI-only feature gate, required by NFR4 / AC8):
  ```
  CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml --no-default-features
  ```
- Expected: exit code 0, no errors and no new warnings in the two modified
  files.

The `webview-ts` component (`bun run typecheck` / `bun test` / `bunx biome
check .`) is deliberately NOT run: this feature touches no TypeScript, and
NFR3 bounds the change surface to two Rust files. A TypeScript diff would
itself be a scope violation.

## Test Verification

- Command:
  ```
  CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib
  ```
- Expected: exit code 0; all pre-existing tests still pass; no test is deleted
  other than the two assertions rewritten by FR5 / FR6.
- Coverage target: not applicable — this project configures no coverage
  tooling. Coverage is expressed instead as the requirement-to-scenario mapping
  in "Functional Requirements Coverage" below, and every FR/NFR is mapped.
- Note: unit tests live in the library target. A run scoped to the binary
  target reports zero tests and verifies nothing.

### Test Scenarios from SPEC.md

| ID | Scenario | Expected Result | Test Type |
|----|----------|-----------------|-----------|
| TS1 | One OSC 9 carrying title `Build done` and body `all green` | The test sink's receive log holds exactly one entry, with that title and that body | Unit |
| TS2 | OSC 9 with no separator, after a title was set by an earlier OSC 2 | The title received by the test sink falls back to the preceding OSC 2 title; the body is the payload (existing test, body unmodified) | Unit |
| TS3 | OSC 133 dispatched twice (a prompt-start form and an exit-code form) | The shared callback state's title and viewer-request queue are unchanged AND the test sink receives zero entries | Unit |
| TS4 | Two identical consecutive OSC 9 pairs; a re-send after the injected clock advances past the window; three mutually distinct pairs | 1 receipt, then 2 receipts, then 3 receipts respectively (the three existing rate-limiter tests, bodies unmodified) | Unit |
| TS5 | Build/feature-gate level: the default-feature library test run and the CLI-only check | Both commands exit 0 (covers AC4 and AC8) | Build |

## Code Quality Verification

- Format:
  ```
  cargo fmt --manifest-path src-tauri/Cargo.toml --check
  ```
  | ID | Check | Expected |
  |----|-------|----------|
  | FMT1 | Formatting of the two modified files | No drift reported for `src-tauri/src/callbacks.rs` or `src-tauri/src/callbacks/tests.rs`. Pre-existing drift in files this feature did not touch is REPORTED, not fixed — reformatting untouched files would violate NFR3 |

- Static source checks (absence properties — see IMPLEMENTATION.md D5; run
  from the project root, `grep -rn` is an acceptable substitute for `rg`):

  | ID | Check | Command | Expected |
  |----|-------|---------|----------|
  | SRC1 | The removed identifier is gone from both feature files | `rg -n 'pending_notifications' src-tauri/src/callbacks.rs src-tauri/src/callbacks/tests.rs` | No matching lines. `src-tauri/src/app/mod.rs` is deliberately excluded from the search — its same-named local variable is out of scope (NFR3) and must still be present |
  | SRC2 | The diverged doc comment is gone | `rg -n 'Pending OSC 9 notifications' src-tauri/src/callbacks.rs` | No matching lines. The search is deliberately narrow: sibling fields' doc comments that mention a tab-side drain describe drains that really exist and must survive |

## SPEC.md Compliance

### Success Criteria

| ID | Criterion | How to Verify |
|----|-----------|---------------|
| AC1 | `pending_notifications` remains in neither `callbacks.rs` nor `callbacks/tests.rs` | SRC1 |
| AC2 | Repeated OSC 9 with distinct pairs leaves no in-process buffer retaining the raw strings | SRC1 (identifier gone) + TS5 (compiles without the field) + M2 (diff review: no replacement structure) |
| AC3 | No doc comment describing a non-existent drain contract remains | SRC2 + M2 |
| AC4 | The library test run passes | TS5 |
| AC5 | The single-OSC-9 test proves the sink received `("Build done", "all green")` exactly once | TS1 |
| AC6 | The OSC 133 no-op test proves the sink received nothing | TS3 |
| AC7 | The three rate-limiter tests pass unmodified | TS4 + M3 (their bodies are unchanged in the diff) |
| AC8 | The CLI-only check passes | TS5 |

### Functional Requirements Coverage

| Requirement | Tasks | Verification |
|-------------|-------|--------------|
| FR1 | task0001 | TS1, TS5, SRC1 |
| FR2 | task0001 | TS5, SRC1 |
| FR3 | task0001 | SRC2 |
| FR4 | task0001 | TS1, TS2, TS4 |
| FR5 | task0001 | TS1, TS2, SRC1 |
| FR6 | task0001 | TS3, SRC1 |
| FR7 | task0001 | TS4 |
| NFR1 | task0001 | TS1, TS2, TS3, TS4, M4 |
| NFR2 | task0001 | SRC1, M2 |
| NFR3 | task0001 | M1 |
| NFR4 | task0001 | TS5 |
| NFR5 | task0001 | TS5, M3, FMT1 |

## Manual Testing (E2E Not Possible)

No E2E framework is configured in this project, and the notification path ends
at the desktop notification service, which no automated test in this repository
can observe. The items below are diff reviews and one optional runtime smoke
check.

- [ ] M1: The integrated diff touches only `src-tauri/src/callbacks.rs` and
      `src-tauri/src/callbacks/tests.rs`. No other file — in particular not
      `src-tauri/src/app/mod.rs`, not `src-tauri/src/tabs/`, not any earlier
      feature's documents — appears in the diff. (NFR3)
- [ ] M2: In the diff, the shared callback-state struct gains no field, and the
      OSC 9 emit branch retains only the inline synchronous sink delivery — no
      bounded vector, ring buffer, or cache was introduced as a replacement,
      and no sibling field's doc comment was removed. (NFR2, AC2, AC3)
- [ ] M3: The rewritten tests still live in the co-located test module, assert
      on the sink's receive log rather than on state internals, and keep the
      existing `<subject>_<scenario>_<expected>` naming; the fallback-title test
      and the three rate-limiter tests appear in the diff with unchanged bodies
      (or do not appear at all). (NFR5, AC7)
- [ ] M4 (optional runtime smoke — requires a GUI build the user launches; not
      required for AC sign-off, since AC1-AC8 are fully covered above): with the
      terminal running, emit one OSC 9 sequence and observe that exactly one
      desktop notification appears with the expected title and body, and that a
      second identical sequence within one second produces none. (NFR1, FR4)

## Security Verification

| Item | Check |
|------|-------|
| Unbounded growth of attacker-influenceable data | SRC1 + M2: the accumulation path is gone and nothing replaced it (OBJ1, FR1, FR2) |
| Retention of raw notification content in process memory | SRC1 + TS5 + M2: after delivery no process-local structure holds the strings (OBJ2, NFR2, AC2) |
| Log redaction unchanged | TS4 + M2: the suppression path and its redacted warn line are untouched (FR7) |

Out of scope for this feature's security verification: log-side redaction and
markup escaping (delivered by earlier features), and zeroing-on-drop of
notification strings.

## Performance Verification

Not applicable — the SPEC states no performance goal, and delivery timing is
explicitly unchanged (FR4, NFR1). The pre-existing property that delivery
blocks the callback thread is a known, accepted characteristic recorded in
IMPLEMENTATION.md D3, not a regression introduced here.

## Verification Summary

| Category | Items | Automated | E2E | Manual |
|----------|-------|-----------|-----|--------|
| Unit test scenarios (TS1-TS4) | 4 | 4 | 0 | 0 |
| Build / feature-gate (TS5) | 1 | 1 | 0 | 0 |
| Static source checks (SRC1-SRC2) | 2 | 2 | 0 | 0 |
| Format check (FMT1) | 1 | 1 | 0 | 0 |
| Diff review (M1-M3) | 3 | 0 | 0 | 3 |
| Optional runtime smoke (M4) | 1 | 0 | 0 | 1 |
| **Total** | **12** | **8** | **0** | **4** |
