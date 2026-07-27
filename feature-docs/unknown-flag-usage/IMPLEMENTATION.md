# Implementation Plan: Unknown-flag usage error

## Overview

Top-level argument handling gains a classification step so that unrecognized
`-`-leading arguments produce a usage error instead of falling through to the
terminal GUI, and `--help` / `-h` print the usage. The classification is a pure
function in the library crate; `src-tauri/src/main.rs` only performs the
resulting side effects.

## Technology Stack

- **Language**: Rust (existing `emterm` crate).
- **Key libraries**: none added. No argument-parsing crate is introduced, so
  there is no new dependency and no new license obligation against the
  project's MIT license.

## Layer Structure

| Layer | Responsibility | Allowed dependencies |
|-------|----------------|----------------------|
| Library crate (`emterm`, `src-tauri/src/lib.rs` roster) | Pure argument classification and the usage text; no I/O, no process exit | std only |
| Binary (`src-tauri/src/main.rs`) | Calls the classifier, writes to stdout/stderr, exits | library crate |

The library module must build in BOTH the default (`gui`) and
`--no-default-features` configurations, so it is declared outside the
`#[cfg(feature = "gui")]` block of the module roster. Its recognized-flag set
varies by the `gui` feature (see Shared Components), which is the only place
the feature gate appears inside the module.

This mirrors the existing `emterm::backend_select` arrangement: the binary
target has no test harness, so decision logic lives in the library where
`cargo test --lib` reaches it.

## Shared Components

| Component | Responsibility | Contract (pre/postcondition) | Used by tasks |
|-----------|----------------|------------------------------|---------------|
| Argument classifier | Classify the top-level argument list | **Pre**: receives the argument list excluding the program name, after bare-word subcommand dispatch has already declined it. **Post**: returns exactly one of three outcomes — *help requested*, *unrecognized argument* (carrying the offending argument string), or *proceed*. Pure: no I/O, no exit, no logging. | task0001 |
| Recognized-flag table | Single definition of the flags this build accepts and whether each consumes a following value | **Pre**: none. **Post**: for the `gui` build, exposes the five child-window flags with their value arity (`--viewer`, `--image-viewer`, `--data-viewer`, `--html-viewer` take one value each; `--settings` takes none); for the CLI-only build, exposes an empty set. Consumed by both the classifier and, as the drift guard, by whatever asserts `run_gui`'s branches stay in sync. | task0001 |
| Usage text | Provide the build-appropriate usage string | **Pre**: none. **Post**: returns a string listing the bare-word subcommands, plus (on the `gui` build) the recognized child-window flags, plus the `-h, --help` line, and ending with the existing per-subcommand help guidance line. No trailing process side effects. | task0001 |

## Conventions

- **Naming**: follow the existing crate style — snake_case module and function
  names, an enum for the classification outcome with one variant per outcome
  in the contract above.
- **Error output**: the unrecognized-argument report is a single line naming
  the offending argument verbatim, followed by the usage text, all on stderr.
  Nothing goes to stdout on the error path.
- **Exit codes**: 2 for the usage error (matching every existing usage error in
  `src-tauri/src/main.rs`), 0 for `--help`.
- **Logging**: the classification path performs no logging. It runs before
  `logging::init()`, so calling the `log` macros there would be a no-op and is
  forbidden by FR5's ordering requirement.

## Cross-task Design Decisions

### D1: Classification lives in the library, side effects in the binary

**Decision**: `src-tauri/src/main.rs` keeps only the call plus the
`println!` / `eprintln!` / `std::process::exit` statements; every decision is
made by the library function.

**Rationale**: NFR1 — the binary target has no test harness, so logic placed
there is untestable. The existing `backend_select` module set this precedent.

**Affected tasks**: task0001.

### D2: Single recognized-flag table, consumed by both the classifier and the dispatcher

**Decision**: the set of recognized flags (and each flag's value arity) is
defined once. The classifier reads it; `run_gui`'s existing branches are made
to agree with it, verified by a test rather than by convention.

**Rationale**: NFR3 — a future flag added to `run_gui` but not to the table
would be rejected before `run_gui` ever sees it, which is a silent breakage of
a working feature. Making the drift mechanically detectable is the whole point
of centralizing the table.

**Affected tasks**: task0001.

### D3: Help wins over unknown

**Decision**: a `--help` / `-h` anywhere in the argument list produces the help
outcome even when an unrecognized argument appears earlier.

**Rationale**: FR2 states the precedence explicitly. A user who mistypes and
then asks for help should get help, not an error.

**Affected tasks**: task0001.

### D4: Values of recognized flags are never classified

**Decision**: when a value-taking recognized flag is seen, the immediately
following argument is skipped without evaluation, even if it starts with `-`.

**Rationale**: FR3 / assumption A4 — `run_gui` already reads `args[pos + 1]`
verbatim as a payload path. Classifying that value would break payload paths
that legitimately begin with `-`, and would diverge from what the dispatcher
actually does with them.

**Affected tasks**: task0001.

### D5: Bare-word subcommand dispatch is untouched

**Decision**: the classifier is invoked only after the existing bare-word
subcommand match in `main` has declined the arguments; the subcommand branches
themselves are not modified.

**Rationale**: FR6 — `emterm markdown --help` must continue to reach the
subcommand's own help, not the top-level one.

**Affected tasks**: task0001.

## Risk Assessment

| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|------------|
| A recognized flag is added to `run_gui` later without updating the table, silently breaking that flag | Medium | High | D2: one table, plus a test that fails when the dispatcher and the table disagree |
| The unmerged `version-flag` branch touches the same dispatch site, producing a merge conflict | High | Low | The table is the only place `--version` needs to be added; SPEC.md Assumption A2 records this for whoever merges |
| Feature-gated differences between the `gui` and CLI-only builds go untested | Medium | Medium | Unit tests exist for both configurations; the CLI-only test command is in `project.components.cli` |
| A payload path starting with `-` is rejected | Low | Medium | D4 |

## Open Questions

None.
