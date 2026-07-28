# Feature: --version Flag Classification and Usage Listing

## Overview

`--version` was added by the `version-flag` feature and the top-level flag
classifier (`RECOGNIZED_FLAGS` / `classify()`) was added by the
`unknown-flag-usage` feature. Each was correct in isolation, but at the merge
point `--version` was never registered in the classifier's table. As a result
`emterm --help` does not list `--version`, and a `--version` appearing anywhere
other than the first argument position is classified as `Unknown`, producing
`emterm: unrecognized argument '--version'` on stderr with exit code 2.

This feature registers `--version` in `RECOGNIZED_FLAGS` for both the `gui` and
CLI-only builds, adds it to `usage_text()` for both builds, and introduces a way
to express a flag that the classifier accepts but that opens no child window, so
`run_gui()` cannot dispatch it.

## Objectives

- List `--version` in the Options section of `emterm --help` on both builds.
- Stop `--version` from being classified as an unrecognized argument at any
  argument position.
- Preserve the `RECOGNIZED_FLAGS` single-source-of-truth structure shared by
  `classify()` and `run_gui()`.
- Keep `emterm --version` behaving exactly as it does today.

## User Stories

### US1: Discover `--version` from help output
As an eMterm user, I want `--version` to appear in `emterm --help`, so that I
can discover the flag without reading the source.

**Acceptance Criteria:**
- [ ] `usage_text()` on the `gui` build contains a `--version` line in Options.
- [ ] `usage_text()` on the CLI-only build contains a `--version` line in Options.
- [ ] The existing `Run \`emterm <subcommand> --help\` for details.` guidance line
      is still present on both builds.

### US2: `--version` is accepted at any argument position
As an eMterm user, I want `emterm --settings --version` not to fail, so that
argument ordering does not produce a surprising `unrecognized argument` error.

**Acceptance Criteria:**
- [ ] `classify(["--settings", "--version"])` returns `Proceed` on the `gui` build.
- [ ] `classify(["--version"])` returns `Proceed` on both builds.
- [ ] No `emterm: unrecognized argument '--version'` is emitted for any argument
      list whose only unrecognized-looking token is `--version`.

### US3: `--version` still prints the version and exits 0
As a packager or script author, I want `emterm --version` to keep printing the
crate version and exiting 0, so that existing install checks keep working.

**Acceptance Criteria:**
- [ ] `emterm --version` writes the crate version plus one newline to stdout.
- [ ] `emterm --version` exits with status 0 and writes nothing to stderr.
- [ ] `emterm --version` does not create the application log directory.
- [ ] All five existing `--version` integration tests in `cli_subcommands` pass.

## Technical Requirements

### Functional Requirements

- **FR1:** `RECOGNIZED_FLAGS` on the `gui` build includes an entry for
  `--version` with `takes_value: false`.
- **FR2:** `RECOGNIZED_FLAGS` on the CLI-only build (`--no-default-features`)
  includes an entry for `--version` with `takes_value: false`. The table is no
  longer empty on that build.
- **FR3:** `RecognizedFlag` can express a recognized flag that has no child
  window to dispatch to, and `run_gui()` skips such entries when scanning for a
  flag to dispatch. `--version` is registered this way.
- **FR4:** `usage_text()` on the `gui` build lists `--version` in the Options
  section, using the same column alignment as the surrounding lines.
- **FR5:** `usage_text()` on the CLI-only build lists `--version` in the Options
  section, using the same column alignment as the surrounding lines.
- **FR6:** `classify()` returns `Proceed` (not `Unknown`) for any argument list
  in which `--version` is the only `-`-leading token that is not otherwise
  recognized, regardless of its position.

### Non-Functional Requirements

- **NFR1 - Maintainability:** `RECOGNIZED_FLAGS` remains the single source of
  truth for the flags `classify()` accepts and the flags `run_gui()` dispatches.
  No second list of flag names is introduced in `main.rs`.
- **NFR2 - Compatibility:** Both the `gui` build and the CLI-only build compile
  and pass their tests. The `--version` early-exit path in `main.rs` still runs
  before `logging::init()` and outside every feature gate.
- **NFR3 - Test integrity:** Existing `arg_dispatch` unit tests that pin the
  table's contents are updated to the new expected contents rather than deleted
  or weakened; the five `--version` integration tests are left unmodified.

## Implementation Approach

### Architecture

The change is confined to the top-level argument entry point:

```
src-tauri/src/main.rs            — process entry: --version early exit (args[1] only),
  └─ emterm::arg_dispatch        — subcommand dispatch, classify(), run_gui()
       ├─ RECOGNIZED_FLAGS       — SSOT table of accepted top-level flags
       ├─ RecognizedFlag         — name / takes_value / (gui) dispatch target
       ├─ classify()             — pure Help | Unknown | Proceed decision
       └─ usage_text()           — build-appropriate usage string
```

### Data Flow

```
argv → main(): args[1] == "--version" ? → print version, exit 0
     → main(): bare-word subcommand? → cli::run / mux::cli::run
     → arg_dispatch::classify(&args[1..])
          ├─ Help    → println!(usage_text()), exit 0
          ├─ Unknown → eprintln!(unrecognized), eprintln!(usage_text()), exit 2
          └─ Proceed → logging::init(), run_gui(args)
                          └─ iterate RECOGNIZED_FLAGS entries that HAVE a
                             dispatch target; --version has none → skipped
```

### Component Design

`RecognizedFlag.target` is currently `GuiTarget` (gated behind
`#[cfg(feature = "gui")]`). Making a recognized flag non-dispatching requires
that field to be able to say "no target". The primary approach is
`target: Option<GuiTarget>`, with `run_gui()` skipping entries whose target is
`None`. Any alternative representation is acceptable provided it satisfies FR3
and NFR1 — the flag set stays in one table and `run_gui()` derives its dispatch
set from that same table.

`--version` entry (gui build):

```
RecognizedFlag { name: "--version", takes_value: false, target: None }
```

`--version` entry (CLI-only build) has no `target` field, matching the existing
`#[cfg]` shape of the struct.

### Usage Text

The `--version` line joins the Options block on both builds, aligned with the
existing entries. Example for the `gui` build:

```
Options:
  --viewer <path>        Open the Markdown viewer window
  --image-viewer <path>  Open the image viewer window
  --data-viewer <path>   Open the JSON/YAML data viewer window
  --html-viewer <path>   Open the HTML viewer window
  --settings             Open the settings window
  --version              Print the version
  -h, --help             Print this help
```

CLI-only build:

```
Options:
  --version              Print the version
  -h, --help             Print this help
```

### Dependencies

**Internal Dependencies:**
- `emterm::arg_dispatch`: the module being changed.
- `src-tauri/src/main.rs` `run_gui()`: consumer of `RecognizedFlag.target`.

**External Dependencies:**
- None. No new crates.

### File Structure

```
src-tauri/src/
├── arg_dispatch.rs      # RECOGNIZED_FLAGS, RecognizedFlag, classify(), usage_text(), unit tests
└── main.rs              # --version early exit, classify() call sites, run_gui() dispatch loop
```

## Test Scenarios

### Unit Tests

- [ ] `classify(["--version"])` returns `Proceed` on the `gui` build.
- [ ] `classify(["--settings", "--version"])` returns `Proceed` on the `gui` build.
- [ ] `classify(["--version", "--typo"])` returns `Unknown("--typo")` — `--version`
      does not consume the following argument (`takes_value: false`).
- [ ] `classify(["--version"])` returns `Proceed` on the CLI-only build.
- [ ] `classify(["--settings"])` still returns `Unknown("--settings")` on the
      CLI-only build.
- [ ] `usage_text()` contains a `--version` line on the `gui` build.
- [ ] `usage_text()` contains a `--version` line on the CLI-only build.
- [ ] The table-contents tests are updated to the new expected flag names on both
      builds and pass.
- [ ] `classify(["--version", "--help"])` returns `Help` — help still wins.

### Integration Tests

- [ ] The five existing `--version` tests in
      `src-tauri/tests/cli_subcommands.rs` pass unmodified.

### E2E Tests

**Existing E2E tests**: None detected.
**Run command**: Not detected.

### Edge Cases

- [ ] `--version` as the value of a value-taking flag (`emterm --viewer --version`)
      is consumed as the payload path and never classified (existing D4 behavior,
      unchanged).
- [ ] `emterm --version anything` still exits 0 with the version on stdout
      (existing `args[1]`-only early-exit behavior, unchanged).

### Performance Tests

Not applicable.

## Security Considerations

Not applicable — the change classifies command-line arguments and emits static
text. No new input is stored, executed, or forwarded.

## Error Handling

| Condition | Behavior | Exit code |
|-----------|----------|-----------|
| `--help` / `-h` anywhere | usage text to stdout | 0 |
| Unrecognized `-`-leading argument | `emterm: unrecognized argument '<arg>'` plus usage text to stderr | 2 |
| `--version` as `args[1]` | crate version to stdout | 0 |
| `--version` elsewhere | recognized, classification proceeds | (path-dependent) |

## Success Criteria

- [ ] All functional requirements are implemented and tested.
- [ ] All test scenarios pass on the `gui` build and the CLI-only build.
- [ ] `cargo check --no-default-features` succeeds.
- [ ] Existing `--version` integration tests pass unmodified.
- [ ] `RECOGNIZED_FLAGS` remains the only flag list (NFR1 verified by inspection
      of `run_gui()`).

## Assumptions

Recorded because this feature was specified in batch mode without user dialogue.

- **A1:** A `--version` appearing at a position other than `args[1]` is only
  required to be *classified* as recognized; it does not have to print the
  version. The `main.rs` early-exit path continues to inspect `args[1]` only, so
  `emterm --settings --version` opens the settings window rather than printing
  the version. Basis: the task's acceptance criteria require only that it not be
  treated as unrecognized, and the `version-flag` feature's D1 constrains
  `--version` handling to run before `logging::init()`.
- **A2:** The non-dispatching flag is expressed as `target: Option<GuiTarget>`.
  Basis: it keeps the flag set in one table (NFR1) with the smallest change. An
  alternative representation satisfying FR3 and NFR1 is acceptable.
- **A3:** The help text for the flag is `Print the version`. Basis: parallel to
  the existing `Print this help` wording; the task places output-format changes
  out of scope, which concerns the `--version` output itself, not the help line.
- **A4:** The `--version` line is placed immediately above `-h, --help` in the
  Options block on both builds. Basis: conventional ordering; no requirement
  specifies a position.

## Open Questions

None. All requirements are `ok`; the decisions taken without the user are
recorded under Assumptions above.

## References

- Notion task: https://app.notion.com/p/3aa3509ec8ee817fb246d1ea56e3c57a
- `feature-docs/version-flag/SPEC.md` — the `--version` flag (D1: pre-logging
  early exit).
- `feature-docs/unknown-flag-usage/SPEC.md` — flag classification framework
  (D2 / NFR3: `RECOGNIZED_FLAGS` as SSOT, D4: value consumption).
- `src-tauri/src/arg_dispatch.rs` — module being changed.
- `src-tauri/src/main.rs` — `--version` early exit and `run_gui()` dispatch loop.
