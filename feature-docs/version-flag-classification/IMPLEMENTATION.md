# Implementation Plan: --version Flag Classification and Usage Listing

## Overview

Register `--version` in the top-level flag table that `classify()` and
`run_gui()` share, and list it in the usage text, on both the `gui` and
CLI-only builds — without letting `run_gui()` treat it as a child-window flag.

## Technology Stack

- **Language**: Rust (existing crate `emterm`, `src-tauri/`).
- **Key libraries**: none added. The change uses only the crate's existing
  argument-handling module and its consumer in the binary entry point.

## Layer Structure

Two layers participate, and the dependency direction is one-way:

| Layer | Element | Responsibility |
|-------|---------|----------------|
| Library (`emterm::arg_dispatch`) | flag table, `classify()`, `usage_text()` | Pure decisions and static text. No I/O, no exit, no logging. Unit-testable via `cargo test --lib`. |
| Binary (`src-tauri/src/main.rs`) | `--version` early exit, `classify()` call sites, `run_gui()` | Side effects only: stdout/stderr writes, exit codes, window dispatch. Reads the library's table; never defines a second flag list. |

The binary depends on the library. The library never depends on the binary.

## Shared Components

| Component | Responsibility | Contract (pre/postcondition) | Used by tasks |
|-----------|----------------|------------------------------|---------------|
| Recognized-flag table (`RECOGNIZED_FLAGS`) | Single source of truth for the flags this build accepts | Precondition: entries are static and build-specific. Postcondition: `classify()` accepts exactly these flags; `run_gui()` dispatches exactly the subset of these entries that declares a child-window target | task0001 |
| Recognized-flag entry (`RecognizedFlag`) | Describes one accepted flag: its name, whether it consumes the next argument, and — on the `gui` build — whether it has a child window to open and which one | Precondition: on the `gui` build the dispatch-target field must be able to express "no target". Postcondition: an entry with no target is accepted by `classify()` and skipped by `run_gui()`'s dispatch scan | task0001 |
| `usage_text()` | Build-appropriate usage string, shared verbatim by the help, unrecognized-argument, and CLI-only-fallthrough call sites | Postcondition: on both builds the Options block lists every user-facing top-level flag, and the trailing subcommand-help guidance line is preserved | task0001 |

## Conventions

- **Single list rule**: flag names appear as string literals in exactly one
  place — the table. Neither `run_gui()` nor any other call site may
  reintroduce a hardcoded flag-name list. (Carried over from the
  `unknown-flag-usage` feature's D2 / NFR3; restated here because this change
  touches the type that enforces it.)
- **Build gating**: the dispatch-target concept exists only on the `gui`
  build. The CLI-only variant of the table and of the entry type keeps its
  existing `#[cfg]` shape; the new entry is added to both variants because
  `--version` is handled outside every feature gate.
- **Error handling**: unchanged. Help wins over an unrecognized argument;
  an unrecognized argument prints the message plus usage to stderr and exits
  2; anything else proceeds.
- **Test discipline**: the two existing tests that pin the table's contents
  are updated to the new expected contents. They are not deleted, and their
  assertions are not loosened into "contains" checks — pinning the exact set
  is the mechanism that catches the class of drift this feature is fixing.

## Cross-task Design Decisions

### D1: Non-dispatching flags are expressed inside the existing table

`--version` is accepted by the classifier but opens no window. Rather than
adding a second list of "recognized but non-dispatching" flag names — which
would recreate exactly the two-list drift that produced this bug — the entry
type gains the ability to say "this flag has no dispatch target", and
`run_gui()`'s scan skips such entries. The table stays the only flag list.

Affected: task0001. Rationale: preserves NFR1 with the smallest change and
gives future non-window flags a defined shape.

### D2: The `args[1]`-only early exit is left alone

Printing the version stays in the binary's pre-logging early exit, which
inspects only the first argument. This feature makes `--version` *classifiable*
at any position; it does not make it *actionable* at any position. Changing
the early exit to scan the whole argument list would move version printing
relative to subcommand dispatch and risks the pre-logging guarantee that the
`version-flag` feature pinned with a test.

Affected: task0001. Consequence, stated so it is not mistaken for a defect:
`emterm --settings --version` opens the settings window and prints nothing.
Recorded as assumption A1 in SPEC.md.

### D3: No new dependencies

The change adds no crates, so there is no license constraint to evaluate
against the project's MIT license.

## Risk Assessment

| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|------------|
| Changing the entry type's target field breaks `run_gui()`'s dispatch match | Medium | High — the terminal or a child window would fail to launch | The consumer is updated inside the same task; the compiler enforces exhaustiveness on the dispatch match |
| A non-dispatching entry is accidentally dispatched, opening a window for `--version` | Low | High — `emterm --version` at a non-first position would launch a window instead of proceeding | `run_gui()` skips entries without a target; a unit test asserts the table's non-dispatching entries |
| The CLI-only table becoming non-empty breaks the test asserting it is empty | High (expected) | Low | That test is rewritten to assert the new expected contents |
| Usage-text alignment drifts from the surrounding lines | Low | Low | The new line reuses the existing column layout; a test asserts the flag's presence |

## Open Questions

None.
