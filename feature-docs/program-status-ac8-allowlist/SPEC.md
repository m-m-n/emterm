# Feature: program-status-ac8-allowlist

## Overview

The ac8 guard in the program_status tests decides allowed project-module paths
with an explicit allowlist (`super::agent_status`) instead of a `crate::`
substring check. Its failure message states the invariant it protects:
program_status builds without the `gui` feature.

## Objectives

- The ac8 guard in program_status tests decides allowed project-module paths
  with an explicit allowlist (`super::agent_status`), not with a `crate::`
  substring check.
- The ac8 guard's failure message states the invariant it protects:
  program_status builds without the `gui` feature.

## Acceptance Criteria

### AC-1: Explicit allowlist over every project-module path

Related requirements: FR1, FR2, FR3, FR5, FR6, FR7

- [ ] The ac8 check decides with the explicit allowlist `super::agent_status`
  (segment boundary) over every `crate::` / `super::` occurrence in
  program_status.rs, not with a `crate::` substring check.
- [ ] The real program_status.rs passes.
- [ ] The synthetic rejected cases of FR6 are reported as violations, and the
  allowed cases are not.

### AC-2: Failure message states the invariant

Related requirements: FR4, FR6

- [ ] The ac8 failure message states the invariant it protects:
  program_status builds without the `gui` feature.

## Technical Requirements

### Functional Requirements

- **FR1 - Scan every project-module path occurrence:** The ac8 check scans
  every occurrence of `crate::` and `super::` in `src-tauri/src/program_status.rs`
  (the file read via `include_str!("../program_status.rs")`), on use lines and
  on non-use lines alike. `src-tauri/src/program_status/tests.rs` is not a scan
  target.
- **FR2 - Explicit allowlist:** A `super::` occurrence passes only when it is
  followed by `agent_status` ending at a path-segment boundary (the next
  characters are `::`, or the path ends). `super::agent_status_exit_latch` and
  any other `super::` path are rejected. Every `crate::` occurrence is
  rejected.
- **FR3 - External crate roots on use lines:** Use lines keep the external-root
  check: the first path segment is one of `std`, `base64`, `self`, or `super`
  (with `super` further limited by FR2). `pub use` lines are checked the same
  way as `use` lines.
- **FR4 - Failure message states the invariant:** A violation's failure
  message states that program_status must stay buildable without the `gui`
  feature and names the offending line or path and the allowlist
  (`super::agent_status`). The message no longer says the module "must not
  reach into other project modules".
- **FR5 - Checker extracted into a function:** The check is a test-side
  function that takes a source text and returns the violations found.
  `ac8_the_module_uses_only_always_built_crates` applies it to
  `include_str!("../program_status.rs")` and asserts there are none.
- **FR6 - Checker self-tests:** Unit tests feed synthetic sources to the
  checker.
  - Allowed cases: `use super::agent_status::AgentState;`, `use std::...`,
    `use base64::...`.
  - Rejected cases: `use super::agent_status_exit_latch::X;`,
    `use crate::agent_status::AgentState;`, `pub use super::<other>::X;`, a
    `crate::` path on a non-use line, a `super::<other>` path on a non-use
    line, a use line whose root is another external crate.
  - A self-test asserts that the violation message states the `gui` feature
    invariant.
- **FR7 - Test name kept:** The test keeps the name
  `ac8_the_module_uses_only_always_built_crates`. test-docs records that list
  it are not updated.

### Non-Functional Requirements

- **NFR1 - Change confined to tests:** Changes are confined to
  `src-tauri/src/program_status/tests.rs`. `src-tauri/src/program_status.rs`
  and `src-tauri/src/lib.rs` are not changed.
- **NFR2 - Existing tests stay green:** All existing program_status tests,
  including `ac8_the_module_is_declared_without_a_gui_gate` and the
  `rank_unify_*` tests, keep passing. The CLI-only check
  `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml --no-default-features`
  keeps passing.
- **NFR3 - No new dependencies:** The checker uses only std. No crate is added.

## Implementation Approach

### Checker rules

| Occurrence | Line kind | Result |
|---|---|---|
| `super::agent_status` followed by `::` or path end | any | allowed |
| `super::agent_status_exit_latch` (prefix match only) | any | violation |
| `super::<other>` | any | violation |
| `super::{...}` (grouped import) | any | violation |
| `crate::...` | any | violation |
| first segment `std` / `base64` / `self` / `super` | `use` / `pub use` / `pub(crate) use` | allowed (super limited as above) |
| first segment is any other crate | `use` / `pub use` / `pub(crate) use` | violation |

### Dependencies

**Internal Dependencies:**
- `src-tauri/src/program_status.rs`: scan target, read via
  `include_str!("../program_status.rs")`; not modified.

**External Dependencies:**
- None (std only).

### File Structure

```
src-tauri/src/
├── program_status.rs          # scan target (unchanged)
└── program_status/
    └── tests.rs               # checker function, ac8 test, checker self-tests
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

### Unit Tests

- [ ] TS-1 (AC-1): `ac8_the_module_uses_only_always_built_crates` runs the
  checker on `include_str!("../program_status.rs")` and finds no violation
  (the file's only project path is `use super::agent_status::AgentState;`).
- [ ] TS-2 (AC-1): Allowed synthetic sources
  `use super::agent_status::AgentState;`, `use std::fmt;`,
  `use base64::Engine as _;` return no violation.
- [ ] TS-3 (AC-1): The prefix case `use super::agent_status_exit_latch::X;`
  returns a violation.
- [ ] TS-4 (AC-1): `use crate::agent_status::AgentState;` returns a violation
  (`crate::` is always rejected).
- [ ] TS-5 (AC-1): `pub use super::tabs::X;` returns a violation (`pub use` is
  checked like `use`).
- [ ] TS-6 (AC-1): Non-use lines `let x = crate::foo::bar();` and
  `super::tabs::f();` each return a violation.
- [ ] TS-7 (AC-1): A use line whose root is another external crate
  (e.g. `use winit::event::Event;`) returns a violation.
- [ ] TS-8 (AC-2): A violation's message mentions the `gui` feature invariant
  and the offending path.

### Integration Tests

- [ ] TS-9 (AC-1):
  `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib program_status`
  passes, and
  `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml --no-default-features`
  passes.

### E2E Tests

**Existing E2E tests**: None
**Run command**: Not detected

### Edge Cases

- [ ] `use super::{agent_status::AgentState};` (grouped import) is rejected.
- [ ] `pub(crate) use` lines are checked like `pub use`.

## Assumptions

- **A1:** Every `crate::` / `super::` occurrence in program_status.rs is
  scanned, including comments and non-use lines; only `super::agent_status` at
  a segment boundary passes; `crate::` is always rejected; `pub use` is
  treated as `use`.
- **A2:** The test name `ac8_the_module_uses_only_always_built_crates` is
  kept; `test-docs/osc7501-program-status/task0001.tests.yaml` and
  `test-docs/pane-state-rank-unify/task0001.tests.yaml` are not updated.
- **A3:** The checker is extracted into a function with synthetic-source
  self-tests for allowed and rejected cases; the scan target stays
  program_status.rs, not tests.rs.
- **A4:** The allowlist is fixed to `super::agent_status`; this feature does
  not check that agent_status itself is declared without a gui gate. Gating
  regressions are caught by `cargo check --no-default-features`.
- **A5:** A grouped import such as `use super::{agent_status::AgentState};` is
  rejected (fail-closed), because the occurrence `super::{` is not
  `super::agent_status`. Visibility-qualified use lines (`pub(crate) use`) are
  checked like `pub use`.
- **A6:** The external-root allowlist on use lines (`std`, `base64`, `self`,
  `super`) is kept unchanged apart from the `super::` restriction.
- **A7:** The change is confined to `src-tauri/src/program_status/tests.rs`;
  production code is untouched.

## Success Criteria

- [ ] All functional requirements are implemented and tested
- [ ] All test scenarios pass

## Open Questions

None.
