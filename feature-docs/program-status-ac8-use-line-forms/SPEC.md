# Feature: program-status-ac8-use-line-forms

## Overview

The ac8 allowlist checker is test-only code in `src-tauri/src/program_status/tests.rs`.
It does not yet recognize four use-line forms. This feature extends its use-line
recognizer to cover them: multiple leading attributes on one line, `use` followed
directly by `::` or `{`, string literals inside attributes, and multi-line
attributes. A non-allowlisted use root written in any of these forms is then
rejected.

## Objectives

- Close the remaining use-line forms that the ac8 allowlist checker (test-only,
  `src-tauri/src/program_status/tests.rs`) does not recognize, so that a
  non-allowlisted use root written in any of these forms is rejected.

## User Stories

### US1: Multiple leading attributes are recognized
As a developer maintaining eMterm, I want the ac8 checker to recognize a use line
preceded by several attributes on the same line, so that a non-allowlisted use root
written in that form is rejected.

**Acceptance Criteria:**
- [ ] AC-1: `#[cfg(a)] #[allow(b)] use winit::event::Event;` is rejected with a
  violation reporting use root `winit`. So are variants with `pub` / `pub(crate)`
  and with no whitespace between the attributes. (FR1, FR5)

### US2: `use` followed directly by `::` or `{` is recognized
As a developer maintaining eMterm, I want the ac8 checker to recognize `use::...` and
`use{...}` lines, so that a non-allowlisted use root written in that form is rejected.

**Acceptance Criteria:**
- [ ] AC-2: `use::winit::event::Event;` and `use{winit::event::Event};` are rejected
  as violations. The self-test asserts rejection only. It includes the no-space
  variants, which are not rejected before this change. (FR2, FR5)

### US3: String literals inside attributes do not break bracket tracking
As a developer maintaining eMterm, I want `[` and `]` inside a string literal in an
attribute to leave the attribute's bracket depth unchanged, so that a use statement
after such an attribute is checked.

**Acceptance Criteria:**
- [ ] AC-3: `#[cfg(feature = "]")] use winit::event::Event;` is rejected with a
  violation reporting use root `winit`. (FR3, FR5)

### US4: Multi-line attributes are tracked
As a developer maintaining eMterm, I want an attribute that spans several lines to be
tracked until its closing `]`, so that a use statement on the closing line is checked.

**Acceptance Criteria:**
- [ ] AC-4: A source whose multi-line attribute closes on a line that carries
  `use winit::event::Event;` is rejected, and the violation names that closing line.
  Example source (FR4, FR5):

  ```rust
  #[cfg(all(
      unix,
      feature = "gui"
  ))] use winit::event::Event;
  ```

### US5: Existing expectations stay intact
As a developer maintaining eMterm, I want the existing ac8 tests and allowlist to stay
as they are, so that the checker's current guarantees are kept.

**Acceptance Criteria:**
- [ ] AC-5: All existing `ac8_*` tests pass unchanged, and
  `ac8_the_module_uses_only_always_built_crates` passes on the current
  `program_status.rs`. (FR6)

## Technical Requirements

### Functional Requirements
- **FR1:** Multiple leading attributes on one line. The use-line recognizer skips
  any number of leading `#[..]` attributes on the same line, separated by any
  whitespace or by none, before the optional visibility (`pub` / `pub(..)`) and the
  `use` keyword.
- **FR2:** `use` followed directly by `::` or `{`. A line whose `use` keyword is
  followed directly by `::` or `{` (no whitespace) is recognized as a use line, with
  or without visibility and leading attributes. Root extraction stays as is: the
  first `::`-separated segment of the path. As a result, every `::`-led or `{`-led
  use path is a violation, with or without whitespace after `use`. This includes
  `use ::std::fmt;` and `use {std::fmt};`.
- **FR3:** String literals inside attributes. `[` and `]` inside a plain string
  literal `"..."` within an attribute do not change the attribute's bracket depth.
  `\"` and `\\` inside the literal are escapes. Raw strings (`r".."`, `r#".."#`) and
  char literals are out of scope.
- **FR4:** Multi-line attributes. When a leading `#[` attribute does not close on
  its line, its state (bracket depth and the FR3 string-literal state) carries over
  to the following lines until the matching `]`. Lines inside the open attribute are
  not use lines. The text after the closing `]` on the closing line is checked as a
  use line under FR1-FR3.
- **FR5:** Rejection self-tests. For each of the four forms, at least one self-test
  feeds a synthetic source to `ac8_check_allowlist` and asserts that it is rejected
  as a violation.
- **FR6:** Existing expectations preserved. The allowlist
  (`AC8_ALLOWED_PROJECT_PATH`, `AC8_ALLOWED_USE_ROOTS`), the existing self-test
  constants and assertions, and the existing test names stay unchanged and keep
  passing. `ac8_the_module_uses_only_always_built_crates` keeps passing on
  `program_status.rs`.

### Non-Functional Requirements
- **NFR1 - Change scope:** Changes are confined to
  `src-tauri/src/program_status/tests.rs`. There are no production-code,
  `Cargo.toml` or `lib.rs` changes, and no new crates.
- **NFR2 - Violation message:** Violations for the new forms use the existing
  `ac8_violation` text: the offending line, the allowlist `super::agent_status`, and
  the invariant ``without the `gui` feature``.

### Assumptions
- **A1:** Form 2: every `::`-led or `{`-led use path is a violation, including
  `use ::std::fmt;` and `use {std::fmt};`. Root extraction is unchanged, and the
  form-2 self-tests assert rejection only.
- **A2:** Form 3: only plain `"..."` literals with `\"` / `\\` escapes are
  literal-aware, and their state carries across lines of a multi-line attribute. Raw
  strings and char literals are out of scope.
- **A3:** The form-4 target is a multi-line attribute whose closing line carries the
  use statement. The variant with the use on the line after the attribute is already
  detected and only needs to stay detected.
- **A4:** Each new rejection self-test has a matching allowed-root test where the
  form allows one (forms 1, 3, 4), following the predecessor
  `program-status-ac8-allowlist-gaps` feature.
- **A5:** Inner attributes (`#![..]`) and block comments between an attribute and
  `use` stay out of scope.

## Implementation Approach

### Architecture

**System Architecture:**
```
src-tauri/src/program_status/tests.rs (test-only)
├── ac8 allowlist checker (ac8_check_allowlist)
│   ├── use-line recognizer        <- extended by FR1-FR4
│   ├── root extraction            <- unchanged (FR2)
│   └── ac8_violation text         <- unchanged (NFR2)
└── ac8_* self-tests               <- new rejection / allowed-root tests (FR5, A4)
```

**Component Diagram:**
```
ac8_* self-tests ──> ac8_check_allowlist ──> use-line recognizer ──> root extraction
                                                                       │
                     AC8_ALLOWED_PROJECT_PATH / AC8_ALLOWED_USE_ROOTS <┘
```

### Data Flow

```
source text → per-line scan (open-attribute state carried across lines, FR4)
            → leading #[..] attributes skipped (FR1, FR3)
            → optional visibility → `use` (followed by whitespace, `::` or `{`, FR2)
            → root extraction → allowlist comparison → ac8_violation on mismatch
```

### API Design

Not applicable. The change is confined to test-only checker logic (NFR1).

### Database Schema

Not applicable.

### Dependencies

**Internal Dependencies:**
- `src-tauri/src/program_status.rs`: the module `ac8_the_module_uses_only_always_built_crates`
  checks (FR6).
- `super::agent_status`: the allowlisted project path reported in violations (NFR2).

**External Dependencies:**
- None. No new crates (NFR1).

### File Structure

```
src-tauri/src/
└── program_status/
    └── tests.rs      # ac8 checker and ac8_* self-tests (only file changed, NFR1)
```

## Declared Change Set

This section states the create-plan derivation instead of a hand-authored
list: the feature-specific paths above are derived at create-plan from
every task's `files` entries in `workflow.yaml`
(`references/phases/create-plan-phase.md`).

Every SPEC declares, by default, the following two workflow-generated
entries in addition to the feature-specific paths above:

- `feature-docs/program-status-ac8-use-line-forms/**`
- `test-docs/program-status-ac8-use-line-forms/**`

`feature-docs/program-status-ac8-use-line-forms/**` covers `REQUIREMENTS.md`,
`SPEC.md`, `IMPLEMENTATION.md`, `workflow.yaml`, `phase-state/`, `tasks/`,
`reviews/roundN.yaml`, `VERIFICATION.md`, `retrospect.yaml`, and the design
artifacts the design step produces. These are generated and owned by the
phase documents and by `references/phase-state.md`; this section cites them
and restates none of their rules.

`test-docs/program-status-ac8-use-line-forms/**` covers
`test-docs/program-status-ac8-use-line-forms/{T}.tests.yaml`, the per-task test
record. It is generated and owned by `implement-phase.md`; this section cites it
and restates none of its rules.

These two default entries are part of the declaration unless the SPEC
author explicitly removes them; their absence is never assumed by
silence — removal is a deliberate, explicit narrowing.

This declaration is a SUPERSET assertion: the actual change set observed
at verification time must be CONTAINED IN the declared set, not equal to
it. A feature that produces no implement tasks generates no
`test-docs/program-status-ac8-use-line-forms/` directory at all; the declared
`test-docs/program-status-ac8-use-line-forms/**` entry is still correct in that
case — a declared path that never materializes is not a violation.

## Test Scenarios

### Unit Tests
- [ ] TS-1 (AC-1): Rejected with root `winit`:
  `#[cfg(a)] #[allow(b)] use winit::event::Event;`,
  `#[cfg(a)]#[allow(b)] pub use winit::event::Event;`,
  `#[cfg(a)] #[allow(b)] pub(crate) use winit::event::Event;`.
  Allowed: `#[cfg(test)] #[allow(unused)] use std::fmt;`.
- [ ] TS-2 (AC-2): Rejected (rejection only): `use::winit::event::Event;`,
  `use{winit::event::Event};`, `pub use::winit::event::Event;`,
  `pub(crate)use{winit::event::Event};`. Also rejected, which pins the chosen
  semantics: `use ::std::fmt;`, `use {std::fmt};`.
- [ ] TS-3 (AC-3): Rejected with root `winit`:
  `#[cfg(feature = "]")] use winit::event::Event;`,
  `#[cfg(feature = "[")] use winit::event::Event;`,
  `#[doc = "\"]"] use winit::event::Event;`.
  Allowed: `#[cfg(feature = "]")] use std::fmt;`.
- [ ] TS-4 (AC-4): Rejected, with the violation naming the closing line: a
  multi-line `#[cfg(all(` attribute closing with `))] use winit::event::Event;`.
  Allowed: the same attribute closing with `))] use std::fmt;`; the source below;
  an attribute closing on its own line followed by `use std::fmt;`.

  ```rust
  #[derive(
      Debug,
  )]
  pub struct X;
  ```
- [ ] TS-5 (AC-5): The existing `ac8_*` test suite and
  `ac8_the_module_uses_only_always_built_crates` pass unchanged. Command:
  `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib program_status::tests::ac8`
- [ ] TS-6 (AC-1, AC-3, AC-4, NFR2): Each violation from the TS-1 / TS-3 / TS-4
  rejected sources contains the offending line, `super::agent_status` and
  ``without the `gui` feature``.

### Integration Tests
- None.

### E2E Tests
**Existing E2E tests**: None
**Run command**: Not detected
- [ ] Existing E2E tests pass without regression (not applicable: none exist)

### Edge Cases
- [ ] No whitespace between attributes (`#[cfg(a)]#[allow(b)]`): recognized (TS-1).
- [ ] `use ::std::fmt;` / `use {std::fmt};`: rejected (TS-2, A1).
- [ ] Escaped quote inside an attribute string (`#[doc = "\"]"]`): the literal does
  not end at `\"` (TS-3).
- [ ] Attribute closing on its own line followed by `use std::fmt;`: allowed (TS-4).
- [ ] Multi-line attribute not followed by a use statement (`#[derive(...)]` +
  `pub struct X;`): allowed (TS-4).
- [ ] Out of scope: raw strings, char literals (FR3, A2); inner attributes
  (`#![..]`) and block comments between an attribute and `use` (A5).

### Performance Tests
- Not applicable.

## Security Considerations

- **Authentication:** Not applicable.
- **Authorization:** Not applicable.
- **Input Validation:** Not applicable (the checker reads synthetic test sources and
  `program_status.rs`).
- **Data Protection:** Not applicable.
- **XSS Prevention:** Not applicable.
- **SQL Injection Prevention:** Not applicable.
- **CSRF Protection:** Not applicable.

## Error Handling

### Error Codes

Not applicable. A rejected use line is reported through the existing
`ac8_violation` text (NFR2).

### Error Flow

```
Non-allowlisted use root → ac8_violation (offending line, super::agent_status,
"without the `gui` feature") → assertion failure in the test
```

## Performance Optimization

Not applicable.

## Success Criteria

- [ ] All functional requirements (FR1-FR6) are implemented and tested
- [ ] All test scenarios (TS-1 to TS-6) pass
- [ ] Changes are confined to `src-tauri/src/program_status/tests.rs` (NFR1)
- [ ] Code review is completed

## Open Questions

> **Note**: 未解決の要件は workflow.yaml で `status: tbd` として管理されています。
> plan フェーズの実行前に解決してください。

- None.

## Implementation Phases (if applicable)

Not applicable.

## References

- ac8 checker and self-tests: `src-tauri/src/program_status/tests.rs`
- Module under check: `src-tauri/src/program_status.rs`
- Predecessor test record: `test-docs/program-status-ac8-allowlist-gaps/task0001.tests.yaml`
