# Feature: program-status-ac8-attribute-comments

## Overview

The `program_status` ac8 allowlist checker in `src-tauri/src/program_status/tests.rs`
scans attributes to decide whether each attribute is open or closed, and carries an
open attribute to the next line. This feature makes the attribute scanner ignore
`[`, `]` and `"` inside line comments and block comments within an attribute, and
reports an attribute that is still open at the end of the source as a violation.

## Objectives

- In the `program_status` ac8 allowlist checker (`src-tauri/src/program_status/tests.rs`),
  comments inside an attribute do not affect whether the attribute is judged open or
  closed, so use lines after such an attribute are still checked against the use-root
  allowlist.

## User Stories

### US1: Comments inside attributes leave the open/close judgement unaffected
As a maintainer of the ac8 allowlist check, I want comments inside an attribute to leave
the attribute's open/close judgement unaffected, so that use lines after such an attribute
are still checked against the use-root allowlist.

**Acceptance Criteria:**
- [ ] AC1: `[`, `]` and `"` inside a line comment or a block comment within an attribute
  do not affect the attribute's open/close judgement. (FR1, FR2)
- [ ] AC2: When the source ends with an attribute still open, the checker reports a
  violation naming the line that holds the attribute's `#[`. (FR3)
- [ ] AC3: Self-tests exist in which the reproduction source (`/* [ */`) and its quote
  variant (`/* " */`) return a winit violation. (FR4)
- [ ] AC4: The expectations of the existing allowlist test and self-tests are unchanged.
  (FR5)

## Technical Requirements

### Functional Requirements
- **FR1: Line comments inside an attribute** — Inside an attribute scanned by
  `ac8_scan_attribute`, a `//` that starts outside a string literal and outside a block
  comment begins a line comment that runs to the end of that physical line. `[`, `]` and
  `"` in the line comment do not change the bracket depth or the string state. The
  line-comment state does not carry to the next line.
- **FR2: Block comments inside an attribute** — Inside an attribute, a `/*` that starts
  outside a string literal and outside a line comment opens a block comment. Block
  comments nest: each `/*` inside a block comment opens one more level and each `*/`
  closes one level; the block comment ends when the level returns to zero. `[`, `]` and
  `"` inside a block comment do not change the bracket depth or the string state. While
  the attribute is open, the block-comment level carries to the next line as part of the
  open-attribute state.
- **FR3: Attribute still open at the end of the source** — When the source ends while an
  attribute is still open (including while inside a string literal or a block comment of
  that attribute), `ac8_check_allowlist` reports one violation built with
  `ac8_violation`. The violation names the trimmed physical line holding the `#[` of the
  attribute still open, and its problem text states that the attribute does not close
  before the end of the source. The violation follows the source-order position of that
  end-of-source check (after every other violation).
- **FR4: Self-tests for the reproduction source** — Self-tests exist in which
  `#[allow(dead_code /* [ */)]\nstruct X;\nuse winit::event::Event;` and the same source
  with `/* " */` in place of `/* [ */` each return a violation reporting use root
  `winit`.
- **FR5: Existing expectations unchanged** — The expectations of
  `ac8_the_module_uses_only_always_built_crates` and of every existing ac8 self-test
  (`ac8_allowlist_*`, `ac8_gaps_*`, `ac8_forms_*`) stay unchanged, and those tests pass.

### Non-Functional Requirements
- **NFR1 - Change scope:** Changes are limited to `src-tauri/src/program_status/tests.rs`.
- **NFR2 - Doc comments match behavior:** The doc comments of `Ac8OpenAttribute`,
  `ac8_scan_attribute`, `ac8_after_leading_attributes` and `ac8_check_allowlist` describe
  the comment handling (FR1, FR2) and the end-of-source rule (FR3) consistently with the
  behavior.
- **NFR3 - Test commands:** Tests run from the project root with
  `CARGO_TARGET_DIR=src-tauri/target` and `--manifest-path src-tauri/Cargo.toml`.

## Implementation Approach

### Architecture

**Component Diagram:**
```
ac8_check_allowlist (per-line loop, carries Ac8OpenAttribute to the next line)
  └─ ac8_after_leading_attributes
       └─ ac8_scan_attribute (bracket depth, string state,
                              line-comment state [FR1], block-comment level [FR2])
  └─ end-of-source check: attribute still open → ac8_violation [FR3]
```

- `Ac8OpenAttribute` (the open-attribute state carried to the next line) holds the
  block-comment level while the attribute is open (FR2). The line-comment state is not
  carried (FR1).
- `ac8_scan_attribute` recognises `//` and `/*` only outside a string literal; inside a
  string literal they are string content (Assumption A1).
- `ac8_check_allowlist` reports the end-of-source violation for an attribute still open,
  naming the trimmed physical line holding that attribute's `#[` (FR3, Assumption A6).

### Data Flow

```
source lines → ac8_check_allowlist → ac8_after_leading_attributes → ac8_scan_attribute
             ← use-root violations ← remaining text / open-attribute state
end of source with open attribute → ac8_violation (attribute does not close)
```

### Dependencies

**Internal Dependencies:**
- `src-tauri/src/program_status/tests.rs`: ac8 allowlist checker and its self-tests.

**External Dependencies:**
- None.

### File Structure

```
src-tauri/src/program_status/
└── tests.rs    # ac8 allowlist checker, comment handling, end-of-source rule, self-tests
```

## Declared Change Set

This section states the create-plan derivation instead of a hand-authored
list: the feature-specific paths above are derived at create-plan from
every task's `files` entries in `workflow.yaml`
(`references/phases/create-plan-phase.md`). Per NFR1 the feature-specific path is
`src-tauri/src/program_status/tests.rs`.

Every SPEC declares, by default, the following two workflow-generated
entries in addition to the feature-specific paths above:

- `feature-docs/program-status-ac8-attribute-comments/**`
- `test-docs/program-status-ac8-attribute-comments/**`

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

New self-tests use the name prefix `ac8_comments_acN_` under their own section header
(Assumption A4).

### Unit Tests
- [ ] TS1 (AC1, AC3): `#[allow(dead_code /* [ */)]\nstruct X;\nuse winit::event::Event;`
  - A violation reporting use root `winit`.
- [ ] TS2 (AC1, AC3): `#[allow(dead_code /* " */)]\nstruct X;\nuse winit::event::Event;`
  - A violation reporting use root `winit`.
- [ ] TS3 (AC1): `#[allow(dead_code /* [ */)]\nstruct X;\nuse std::fmt;` - No violation.
- [ ] TS4 (AC1): A multi-line attribute with a line comment holding `]`, `[` or `"`,
  e.g. `#[cfg(all( // ]\nunix))] use winit::event::Event;` - A violation reporting use
  root `winit` (the attribute closes on the second line).
- [ ] TS5 (AC1): A block comment holding `]` that spans lines inside a multi-line
  attribute, e.g. `#[cfg(all( /* ]\n] */ unix))]\nuse winit::event::Event;` - A violation
  reporting use root `winit`.
- [ ] TS6 (AC1): `#[allow(dead_code /* /* */ [ */)]\nstruct X;\nuse winit::event::Event;`
  - A violation reporting use root `winit` (nested block comment, FR2).
- [ ] TS7 (AC1): `#[doc = "/*"] use winit::event::Event;` - A violation reporting use root
  `winit` (a comment opener inside a string literal is string content).
- [ ] TS8 (AC2): An attribute that never closes, e.g. `#[cfg(all(\nunix,\nuse std::fmt;`
  - A violation whose text contains the line `#[cfg(all(`, `super::agent_status` and
  ``without the `gui` feature``.
- [ ] TS9 (AC2): An attribute whose block comment never closes, e.g.
  `#[allow(dead_code /* ]\nuse std::fmt;` - A violation naming the line holding the
  attribute's `#[`.

### Integration Tests
- [ ] TS10 (AC4):
  `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib`
  - `ac8_the_module_uses_only_always_built_crates` and every existing ac8 self-test pass
  with unchanged expectations.

### E2E Tests
**Existing E2E tests**: None
**Run command**: Not detected

### Edge Cases
- [ ] `//` and `/*` inside a string literal of an attribute are string content and do
  not start a comment (TS7, Assumption A1).
- [ ] Nested block comments inside an attribute (TS6, FR2).
- [ ] Source ends inside a block comment of an open attribute (TS9, FR3).

## Assumptions

- **A1:** `//` and `/*` that appear inside a string literal of an attribute are string
  content and do not start a comment.
- **A2:** Comments outside an attribute's `#[..]` (before the first `#[` on a line or
  between attributes on the same line) are out of scope.
- **A3:** Char literals and raw strings inside attributes keep no special treatment.
- **A4:** The new self-tests use the name prefix `ac8_comments_acN_` under their own
  section header.
- **A5:** Block comments inside an attribute nest as in the Rust lexer; the level carries
  across lines while the attribute is open.
- **A6:** The end-of-source violation names the trimmed physical line holding the `#[` of
  the attribute still open, built with `ac8_violation`; its problem text states the
  attribute does not close before the end of the source.

## Success Criteria

- [ ] All functional requirements are implemented and tested
- [ ] All test scenarios pass
- [ ] Code review is completed

## Open Questions

None.

## References

- ac8 allowlist checker: `src-tauri/src/program_status/tests.rs`
