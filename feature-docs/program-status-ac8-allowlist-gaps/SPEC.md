# Feature: program-status-ac8-allowlist-gaps

## Overview

The ac8 allowlist checker in `src-tauri/src/program_status/tests.rs` has two
detection gaps: use-line forms it does not recognize, and an
identifier-continuation judgement that lets combining characters such as
U+0301 through after `super::agent_status`. This feature closes both gaps so
the checker rejects every input in scope that
`cargo check --no-default-features` also rejects.

## Objectives

- Close the two detection gaps left in the program_status ac8 allowlist
  checker (`src-tauri/src/program_status/tests.rs`): unrecognized use-line
  forms, and the identifier-continuation judgement that lets combining
  characters such as U+0301 through.
- The checker rejects every input in scope that
  `cargo check --no-default-features` also rejects.

## Acceptance Criteria

### AC-1: Whitespace forms of use lines

Related requirements: FR1, FR2, FR6

- [ ] The recognizer accepts one or more whitespace characters after `use`,
  `pub` and a visibility qualifier, and also accepts zero whitespace between
  `)` and `use`.
- [ ] Self-tests reject `pub (crate) use winit::event::Event;`,
  `"use\twinit::event::Event;"` and `pub(crate)use winit::event::Event;`.

### AC-2: Fail-closed terminator after `super::agent_status`

Related requirements: FR5, FR6

- [ ] `super::agent_status` is allowed only before end of text, `::`, `;` or
  `,`.
- [ ] A self-test rejects `"use super::agent_status\u{0301}::X;"`.

### AC-3: Bare use line and leading attribute

Related requirements: FR3, FR4, FR6

- [ ] Self-tests reject a bare `use` line whose root `winit` is on the next
  non-blank line, and `#[cfg(feature = "gui")] use winit::event::Event;`.

### AC-4: Existing behaviour preserved

Related requirements: FR6, NFR1, NFR3

- [ ] The existing ac8 and ac8_allowlist self-tests and
  `ac8_the_module_uses_only_always_built_crates` keep passing.

## Technical Requirements

### Functional Requirements

- **FR1 - Whitespace after use / pub / visibility qualifier:** The use-line
  recognizer accepts one or more whitespace characters (space, tab and other
  `char::is_whitespace` characters) after `use`, after `pub`, and after a
  `pub(...)` visibility qualifier. `pub (crate) use winit::event::Event;` and
  `"use\twinit::event::Event;"` are recognized as use lines, and rule B
  rejects them (root `winit` is not one of `std`, `base64`, `self`, `super`).
- **FR2 - pub(...)use with zero whitespace:** A visibility qualifier followed
  directly by `use` with no whitespace between `)` and `use` is recognized as
  a use line. `pub(crate)use winit::event::Event;` is rejected.
- **FR3 - Bare use line:** A line that holds only the `use` keyword (with or
  without a `pub` / `pub(...)` qualifier) is recognized as a use line, and the
  use root is the first path segment of the next non-blank line.
  `"use\nwinit::event::Event;"` is rejected.
- **FR4 - Leading attribute on the same line:** When a line starts with a
  `#[...]` attribute and a use statement follows it on the same line, the
  text after the attribute is checked as a use line.
  `#[cfg(feature = "gui")] use winit::event::Event;` is rejected.
- **FR5 - Fail-closed terminator after super::agent_status:** An occurrence of
  `super::agent_status` is allowed only when the text right after it is end of
  text, `::`, `;` or `,`. Every other following character is rejected,
  including U+0301 and other non-ASCII characters.
  `"use super::agent_status\u{0301}::X;"` is rejected.
- **FR6 - Rejection self-tests and preserved allowed set:** Each form in
  FR1-FR5 has a self-test that feeds a synthetic source to
  `ac8_check_allowlist` and asserts a violation. The existing allowed snippets
  in `ac8_allowlist_ac2_*` still produce no violation, the existing rejected
  snippets still produce violations, and
  `ac8_the_module_uses_only_always_built_crates` still passes against
  `program_status.rs`.

### Non-Functional Requirements

- **NFR1 - Test-only change:** Only `src-tauri/src/program_status/tests.rs`
  changes. `program_status.rs`, `lib.rs` and `agent_status.rs` are unchanged.
- **NFR2 - No new dependencies:** No new crate or dev-dependency is added.
  Tests use the standard libtest harness and follow the existing `#[test]`
  style in tests.rs.
- **NFR3 - Violation message contents:** Violations produced by the newly
  closed forms are built with `ac8_violation`. Each one contains the offending
  line text, the allowlist `super::agent_status`, and the
  `without the `gui` feature` invariant, as the existing `ac8_allowlist_ac6_*`
  tests require.
- **NFR4 - Verification commands:**
  `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib`
  passes, run from the project root.

## Implementation Approach

### Use-line recognition

| Line form | Recognized as use line | Use root | Requirement |
|---|---|---|---|
| `use` / `pub` / `pub(...)` followed by one or more `char::is_whitespace` characters | yes | first path segment after `use` | FR1 |
| `pub(...)use` (zero whitespace between `)` and `use`) | yes | first path segment after `use` | FR2 |
| Line holding only `use` (optionally with `pub` / `pub(...)`) | yes | first path segment of the next non-blank line | FR3 |
| `#[...]` attribute followed by a use statement on the same line | yes (text after the attribute) | first path segment after `use` | FR4 |

Rule B (the external-root check: `std`, `base64`, `self`, `super`) applies to
every recognized use line.

### `super::agent_status` terminator

| Text right after `super::agent_status` | Result | Requirement |
|---|---|---|
| end of text | allowed | FR5 |
| `::` | allowed | FR5 |
| `;` | allowed | FR5 |
| `,` | allowed | FR5 |
| any other character (space, `)`, U+0301, other non-ASCII) | violation | FR5 |

### Dependencies

**Internal Dependencies:**
- `src-tauri/src/program_status.rs`: scan target of
  `ac8_the_module_uses_only_always_built_crates`; not modified.
- `ac8_check_allowlist` / `ac8_violation` in
  `src-tauri/src/program_status/tests.rs`: the checker and violation builder
  this feature extends.

**External Dependencies:**
- None.

### File Structure

```
src-tauri/src/
├── program_status.rs          # scan target (unchanged)
└── program_status/
    └── tests.rs               # checker, ac8 test, checker self-tests
```

## Declared Change Set

This section states the create-plan derivation instead of a hand-authored
list: the feature-specific paths above are derived at create-plan from
every task's `files` entries in `workflow.yaml`
(`references/phases/create-plan-phase.md`).

Every SPEC declares, by default, the following two workflow-generated
entries in addition to the feature-specific paths above:

- `feature-docs/{feature}/**`
- `test-docs/{feature}/**`

`feature-docs/{feature}/**` covers `REQUIREMENTS.md`, `SPEC.md`,
`IMPLEMENTATION.md`, `workflow.yaml`, `phase-state/`, `tasks/`,
`reviews/roundN.yaml`, `VERIFICATION.md`, `retrospect.yaml`, and the design
artifacts the design step produces. These are generated and owned by the
phase documents and by `references/phase-state.md`; this section cites them
and restates none of their rules.

`test-docs/{feature}/**` covers `test-docs/{feature}/{T}.tests.yaml`, the
per-task test record. It is generated and owned by `implement-phase.md`;
this section cites it and restates none of its rules.

These two default entries are part of the declaration unless the SPEC
author explicitly removes them; their absence is never assumed by
silence — removal is a deliberate, explicit narrowing.

This declaration is a SUPERSET assertion: the actual change set observed
at verification time must be CONTAINED IN the declared set, not equal to
it. A feature that produces no implement tasks generates no
`test-docs/{feature}/` directory at all; the declared
`test-docs/{feature}/**` entry is still correct in that case — a declared
path that never materializes is not a violation.

## Test Scenarios

Inputs containing `\t`, `\n` or `\u{0301}` are written as Rust string
literals.

### Unit Tests

- [ ] TS-1 (AC-1): `pub (crate) use winit::event::Event;` - rejected.
- [ ] TS-2 (AC-1): `"use\twinit::event::Event;"` - rejected.
- [ ] TS-3 (AC-1): `pub(crate)use winit::event::Event;` - rejected.
- [ ] TS-4 (AC-1): `"pub\tuse winit::event::Event;"` - rejected.
- [ ] TS-5 (AC-2): `"use super::agent_status\u{0301}::X;"` - rejected.
- [ ] TS-6 (AC-2): `use super::agent_status as status;` and
  `super::agent_status)` (space or `)` after the path) - rejected.
- [ ] TS-7 (AC-2): `use super::agent_status`, `use super::agent_status;`,
  `f(super::agent_status, 1)` and `super::agent_status::AgentState` -
  allowed.
- [ ] TS-8 (AC-3): `"use\nwinit::event::Event;"` - rejected.
- [ ] TS-9 (AC-3): `"pub use\n\nwinit::event::Event;"` (blank line between) -
  rejected.
- [ ] TS-10 (AC-3): `#[cfg(feature = "gui")] use winit::event::Event;` -
  rejected.
- [ ] TS-11 (AC-3): `#[cfg(all(unix, feature = "gui"))] pub use winit::event::Event;`
  (nested parentheses in the attribute) - rejected.
- [ ] TS-12 (AC-4): The existing `ac8_allowlist_ac2_*` allowed snippets, plus
  `#[derive(Debug)]` lines with no use statement - allowed.
- [ ] TS-13 (AC-4): `include_str!("../program_status.rs")` - no violation.

### Integration Tests

- [ ] `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib`
  passes, run from the project root (NFR4).

### E2E Tests

**Existing E2E tests**: None
**Run command**: Not detected

## Assumptions

- **A1:** The closed use-line forms are: `pub (crate) use`, `use<TAB>` (any
  whitespace), a bare `use` line whose root sits on the next non-blank line, a
  leading `#[...]` attribute on the same line, and `pub(...)use` with zero
  whitespace. Each one gets a rejection self-test.
- **A2:** The terminator set after `super::agent_status` is exactly end of
  text, `::`, `;` and `,`. Every other character is rejected, including U+0301
  and other non-ASCII characters.
- **A3:** The checker scans line by line via `str::lines()`, which strips `\n`
  and `\r\n`. So "end of text" means the scanned line ends right after the
  path.
- **A4:** `program_status.rs` currently names `super::agent_status` only as
  `use super::agent_status::AgentState;` (line 28), and its other use roots
  are `base64`. The narrower terminator set therefore keeps
  `ac8_the_module_uses_only_always_built_crates` green.

## Success Criteria

- [ ] All functional requirements are implemented and tested
- [ ] All test scenarios pass

## Open Questions

None.
