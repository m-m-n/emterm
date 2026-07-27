# Feature: Unknown-flag usage error

## Overview

`emterm` currently ignores every `-`-leading argument it does not explicitly
branch on, falling through to the terminal GUI. This feature makes the binary
reject unrecognized `-`-leading arguments with a usage message on stderr and a
non-zero exit code, and adds a top-level `--help` / `-h` that prints the same
usage on stdout with exit 0.

## Objectives

- Never open a window in response to an argument the binary does not
  understand.
- Give the binary a top-level `--help`, which the CLI-only usage text already
  implies exists.
- Keep every currently recognized subcommand and child-window flag working
  unchanged, on both the GUI build and the CLI-only build
  (`--no-default-features`).

## User Stories

### US1: Typo does not open a window
As an eMterm user, I want `emterm --typo` to fail with a usage message, so
that a mistyped invocation does not leave an unwanted terminal window open.

**Acceptance Criteria:**
- [ ] `emterm --typo` writes `emterm: unrecognized argument '--typo'` plus the
      usage text to stderr.
- [ ] The process exits with code 2 and no window is created.
- [ ] Nothing is written to stdout.

### US2: Discovering what the binary accepts
As an eMterm user, I want `emterm --help` to print the usage, so that I can
see the available subcommands and flags without reading the source.

**Acceptance Criteria:**
- [ ] `emterm --help` and `emterm -h` write the usage text to stdout.
- [ ] The process exits with code 0 and no window is created.

### US3: Existing invocations keep working
As eMterm itself (which re-executes its own binary to open child windows), I
want the recognized flags to keep dispatching, so that Markdown / image / data
/ HTML viewers and the settings window still launch.

**Acceptance Criteria:**
- [ ] `emterm` with no arguments starts the terminal GUI.
- [ ] `emterm --viewer <path>`, `--image-viewer <path>`, `--data-viewer
      <path>`, `--html-viewer <path>` and `--settings` dispatch as before.
- [ ] `emterm markdown <file>`, `json`, `yaml`, `image`, `html`,
      `agent-status` and `mux …` dispatch as before.

## Technical Requirements

### Functional Requirements

- **FR1:** When top-level argument parsing runs (i.e. `argv[1]` did not match
  a bare-word subcommand) and the argument list contains a `-`-leading
  argument that is neither a recognized flag nor the value consumed by a
  recognized flag, the binary writes `emterm: unrecognized argument
  '<arg>'` followed by the usage text to **stderr** and exits with code **2**.
  The reported argument is the first such argument in left-to-right order.
- **FR2:** When the argument list contains `--help` or `-h` (and `argv[1]` did
  not match a bare-word subcommand), the binary writes the usage text to
  **stdout** and exits with code **0**. This check takes precedence over FR1,
  so `emterm --typo --help` exits 0.
- **FR3:** The recognized-flag set is build-dependent and is defined in exactly
  one place:
  - GUI build (`feature = "gui"`): `--viewer`, `--image-viewer`,
    `--data-viewer`, `--html-viewer` (each consuming one following value) and
    `--settings` (consuming none).
  - CLI-only build (`--no-default-features`): empty.
  The value consumed by a value-taking flag is never itself evaluated as a
  possible unknown flag, even when it starts with `-`.
- **FR4:** The usage text lists the bare-word subcommands and, on the GUI
  build, the recognized child-window flags. It retains the existing guidance
  line `Run \`emterm <subcommand> --help\` for details.`
- **FR5:** FR1 / FR2 are evaluated before `logging::init()` and before any
  windowing or event-loop construction, so a rejected invocation performs no
  logger installation and creates no window.
- **FR6:** Bare-word subcommand dispatch is unchanged and takes precedence:
  `emterm markdown --help` still reaches `emterm::cli::run` and is handled by
  the subcommand, not by FR2.

### Non-Functional Requirements

- **NFR1 - Testability:** The argument-classification logic lives in the
  library crate (`emterm`), not in `src-tauri/src/main.rs`, so it is covered
  by `cargo test --lib`. `src-tauri/src/main.rs` keeps only the thin call plus
  the `println!` / `eprintln!` / `std::process::exit` side effects. This
  mirrors the existing `emterm::backend_select` arrangement (see the doc
  comment at `src-tauri/src/main.rs:18-23`).
- **NFR2 - Compatibility:** Behavior is identical on Linux and Windows, and
  present in both the default (`gui`) and `--no-default-features` builds. The
  self-exec child-window paths must not regress.
- **NFR3 - Single source of truth:** The recognized-flag set that FR1 checks
  against and the flags `run_gui` actually branches on must not drift: adding
  a flag in one place without the other is a defect. The implementation keeps
  one definition that both consume.

## Implementation Approach

### Architecture

```
argv
  │
  ├─ argv[1] ∈ {markdown, json, yaml, image, html, agent-status}  → cli::run
  ├─ argv[1] == "mux"                                             → mux::cli::run
  │
  └─ otherwise ─► emterm::arg_dispatch::classify(&argv[1..])
                     ├─ Help          → usage to stdout, exit 0
                     ├─ Unknown(arg)  → error line + usage to stderr, exit 2
                     └─ Proceed       → logging::init(); run_gui / CLI-only usage
```

The module name (`arg_dispatch` above) is indicative; the planner picks the
final placement. What is normative is that the classification is a pure
function over `&[String]` returning a small enum, and that it is unit-tested
in the library crate.

### Data Flow

```
main() → classify(args) → { Help | Unknown(String) | Proceed }
                              │        │               │
                        stdout usage   stderr usage    existing path
                        exit 0         exit 2          (logging::init, …)
```

### Classification algorithm

Scanning `argv[1..]` left to right:

1. If the current argument is `--help` or `-h`, the result is `Help`
   (short-circuit; FR2 precedence).
2. If the current argument is a recognized value-taking flag, skip the next
   argument (if any) without classifying it.
3. If the current argument is a recognized valueless flag, continue.
4. If the current argument starts with `-`, the result is `Unknown(arg)` —
   but scanning continues only insofar as needed to honour FR2: a later
   `--help` still wins. Implementation may do a first pass for `--help` / `-h`
   and a second pass for unknown flags, or a single pass that remembers the
   first unknown and returns `Help` if a help flag is seen later.
5. Otherwise (a non-`-` argument that is not consumed as a flag value),
   continue scanning; such arguments do not by themselves cause an error.

`-` alone and `--` alone are `-`-leading and not in the recognized set, so
they classify as `Unknown`.

### Usage text

GUI build:

```
Usage: emterm [options]
       emterm <markdown|json|yaml|html|image> <file> [options]
       emterm agent-status <idle|working|blocked|done|clear> [--name <n>]
       emterm mux <args>...

Options:
  --viewer <path>        Open the Markdown viewer window
  --image-viewer <path>  Open the image viewer window
  --data-viewer <path>   Open the JSON/YAML data viewer window
  --html-viewer <path>   Open the HTML viewer window
  --settings             Open the settings window
  -h, --help             Print this help

Run `emterm <subcommand> --help` for details.
```

CLI-only build:

```
emterm: this build provides only CLI subcommands.
Usage: emterm <markdown|json|yaml|html|image> <file> [options]
       emterm agent-status <idle|working|blocked|done|clear> [--name <n>]
       emterm mux <args>...

Options:
  -h, --help             Print this help

Run `emterm <subcommand> --help` for details.
```

The exact wording is not byte-normative; what is normative is that both builds
list their subcommands, that the GUI build lists its five child-window flags,
and that the `Run \`emterm <subcommand> --help\` for details.` line is present.

### Dependencies

**Internal Dependencies:**
- `src-tauri/src/main.rs`: dispatch site; gains the classification call.
- `src-tauri/src/lib.rs`: module roster; gains the new module (must be
  available in both `gui` and non-`gui` builds, so it is declared outside the
  `#[cfg(feature = "gui")]` block).

**External Dependencies:** none. No argument-parsing crate is introduced.

## Test Scenarios

### Unit Tests (library crate, `cargo test --lib`)

- [ ] TS-1: `classify(["--help"])` → `Help`; `classify(["-h"])` → `Help`.
- [ ] TS-2: `classify(["--typo"])` → `Unknown("--typo")`.
- [ ] TS-3: `classify([])` → `Proceed`.
- [ ] TS-4 (GUI cfg): `classify(["--viewer", "/tmp/p"])` → `Proceed`.
- [ ] TS-5 (GUI cfg): `classify(["--viewer", "--weird"])` → `Proceed` — the
      value is consumed, not classified.
- [ ] TS-6 (GUI cfg): `classify(["--settings"])` → `Proceed`.
- [ ] TS-7: `classify(["--typo", "--help"])` → `Help` (FR2 precedence).
- [ ] TS-8: `classify(["-"])` and `classify(["--"])` → `Unknown`.
- [ ] TS-9: first-unknown ordering — `classify(["--a", "--b"])` →
      `Unknown("--a")`.
- [ ] TS-10 (CLI-only cfg): `classify(["--settings"])` → `Unknown("--settings")`
      on the `--no-default-features` build.

### Integration Tests

- [ ] TS-11: `src-tauri/tests/cli_subcommands.rs` — existing subcommand
      dispatch assertions continue to pass unchanged.

### E2E Tests

**Existing E2E tests**: `src-tauri/tests/cli_subcommands.rs`
**Run command**: `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --test cli_subcommands`
- [ ] Existing E2E tests pass without regression.

### Edge Cases

- [ ] A recognized value-taking flag at the end of the list with no following
      value: classification returns `Proceed`; the existing `run_gui` handlers
      already report the missing payload and exit 2.
- [ ] Mixed `emterm --settings --typo` → `Unknown("--typo")`.
- [ ] A non-`-` stray argument such as `emterm foo` keeps the current behavior
      (GUI starts on the GUI build; CLI-only prints its usage and exits 2).

### Manual Tests

- [ ] MT-1: On a release GUI build, `emterm --typo` prints usage to stderr,
      exits 2, and opens no window.
- [ ] MT-2: On a release GUI build, `emterm --help` prints usage to stdout and
      exits 0.

## Security Considerations

- **Input Validation:** The unrecognized argument is echoed verbatim on a
  single line to stderr; it is never passed to a shell, a filesystem path, or
  a format string as the format argument.
- No other security surface changes.

## Error Handling

| Condition | Stream | Exit code |
|-----------|--------|-----------|
| Unrecognized `-`-leading argument | stderr: error line + usage | 2 |
| `--help` / `-h` | stdout: usage | 0 |
| CLI-only build with no matching subcommand and no `-`-leading argument | stderr: usage (existing behavior) | 2 |

## Success Criteria

- [ ] All functional requirements are implemented and tested.
- [ ] All unit and integration test scenarios pass.
- [ ] `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml` succeeds.
- [ ] `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml --no-default-features` succeeds.
- [ ] No regression in `src-tauri/tests/cli_subcommands.rs`.

## Assumptions

Recorded because this feature was specified in batch mode with no user
available to confirm. The Codex consultation loop was skipped: the `codex`
binary is not installed in this environment.

- **A1:** Top-level `--help` / `-h` is in scope. The task description lists
  `emterm --help` among the symptoms and notes that the CLI-only usage text
  advertises a `--help` that does not exist; rejecting `--help` with a
  non-zero exit would be a worse outcome than implementing it.
- **A2:** `--version` is NOT implemented here. It is owned by the separate
  `version-flag` feature, whose branch
  (`em-workflow/version-flag/integration`) exists but is not merged into
  `main`, and whose REQUIREMENTS.md explicitly scopes this feature out.
  Consequence: between this feature landing and `version-flag` landing,
  `emterm --version` is an unrecognized argument (usage + exit 2). Whoever
  merges `version-flag` adds `--version` to the recognized-flag set of FR3.
- **A3:** The non-zero exit code is **2**, matching every existing usage error
  in `src-tauri/src/main.rs` (missing viewer payload, CLI-only fallthrough).
- **A4:** A value-taking recognized flag consumes the following argument
  unconditionally, so `--viewer --weird` is not an unknown-flag error. This
  matches how `run_gui` already reads `args[pos + 1]` verbatim.
- **A5:** Non-`-` stray arguments are not errors. The task description scopes
  the fix to `-`-leading arguments only.
- **A6:** `-h` is added alongside `--help`. No existing flag uses `-h`.

## Open Questions

None.

## References

- Notion task: [https://www.notion.so/3a83509ec8ee81e2a193ee062c99ab65](https://www.notion.so/3a83509ec8ee81e2a193ee062c99ab65)
- Requirements: `feature-docs/unknown-flag-usage/REQUIREMENTS.md`
- Related feature (owns `--version`, unmerged):
  `feature-docs/version-flag/SPEC.md`
- Dispatch site: `src-tauri/src/main.rs:60-105`, `src-tauri/src/main.rs:107-177`
