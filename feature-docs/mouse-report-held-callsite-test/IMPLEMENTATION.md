# Implementation Plan: mouse-report-held-callsite-test

## Overview

Add call-site fixation tests to the existing `window_host` inline test module so
that the button path (`run_button_decision`) and the wheel path
(`handle_mouse_wheel`) in `pointer_routing.rs` cannot silently stop passing the
live held-button value to the held-aware apply entry points. Production code is
frozen (FR4): the entire change set is additions to
`src-tauri/src/window_host/tests.rs`.

## Technology Stack

- **Language / crate**: Rust, existing `src-tauri` crate, GUI-gated
  `window_host` module (`#[cfg(feature = "gui")]`), so the CLI-only feature gate
  is unaffected (NFR7).
- **Test framework**: the crate's built-in `#[cfg(test)]` harness, exercised
  through the `main` component's test command in `workflow.yaml`. No new test
  binary and no new test-compilation unit (NFR2).
- **Key mechanism**: compile-time source embedding of a same-directory file —
  the mechanism the existing tests in this module already use — plus a
  test-local lexical scanner written against the standard library only (FR5,
  FR6).
- **New dependencies**: **none**. No crate is added, so `project.license`
  (`MIT`) acquires no new obligation and there is no license conflict to
  resolve. Dependency licence ledger for this feature: *empty — nothing added*.

## Layer Structure

| Layer | Contents | Responsibility |
|---|---|---|
| Production (frozen) | `pointer_routing.rs`, `mouse_report.rs`, `event_loop.rs` | Subject of the fixation. Read-only for this feature; not modified, not refactored, not made "more observable" (FR4, NFR5). |
| Test | `src-tauri/src/window_host/tests.rs` | Holds the new scan elements and the new tests, alongside the existing tests which stay byte-identical (FR8, NFR2). |

Allowed dependency direction: **test → production only**, and only in two
forms — the behavioural seams the existing tests already use, and the
production file's own source text consumed as inert data. The reverse
direction does not exist: no production edit may be made to make a test
easier to write.

## Shared Components

This feature decomposes into a single task, so no component crosses a task
boundary and there is nothing to pin here as a cross-task contract.

| Component | Responsibility | Contract (pre/postcondition) | Used by tasks |
|-----------|----------------|------------------------------|---------------|
| *(none)* | — | — | — |

The scan elements (lexical scanner, body extractor, argument splitter,
judgment function) are internal to the single task and their contracts are
stated in `tasks/task0001.md` — they are deliberately *not* duplicated here.

## Conventions

- **Test naming**: `<subject>_<scenario>_<expected>`, per the repository test
  conventions (NFR3). The SPEC's TS entries already carry conforming names; use
  them verbatim so the SPEC ↔ test mapping stays a literal string match.
- **Placement**: append to the existing inline `#[cfg(test)]` module file. Never
  edit, move, re-order or re-word an existing test in that file (FR8, AC-8).
- **Assertion-message policy**: every failure message names (a) the requirement
  ID being violated, (b) the call site by function name, and (c) the token shape
  that was expected versus what was found. A red on a name-coupled scan must
  tell the reader whether the fix is "restore the argument" or "this was a
  legitimate rename — update the search name in the same change" (NFR8).
- **Names the scan is allowed to couple to** (closed set, NFR8): the two
  enclosing function names, the three callee names, and the one field name. The
  host-side argument identifier is **derived from the extracted signature**, never
  hardcoded.
- **Type-naming restriction**: the tests construct and name no runtime type from
  the windowing, GPU, PTY or terminal-core stacks (NFR1). Consequently the
  host-side parameter cannot be selected by its declared type; it is selected by
  membership in the enclosing function's own parameter-identifier set.
- **Error handling in the scan**: a name that cannot be located yields an
  explicit, distinguishable "not found" failure with its own message — never a
  silently empty body that would let an assertion pass vacuously.

## Cross-task Design Decisions

### D1 — One task, no decomposition

The whole change is additions to one file with one shared set of scan elements.
Splitting it would force two worktrees to each author their own copy of the same
scanner in the same file, producing guaranteed duplicate definitions and a
conflict with no correctness benefit. SPEC's *Implementation Phases* reaches the
same conclusion. Affected: task0001.

### D2 — Source-scan, with the NFR5 exception kept narrow

The held pass-through at these two call sites has no observable runtime seam
that a unit test can reach without constructing the windowing stack (NFR1
forbids that), and the earlier attempt to create one was reverted for violating
the then-current NFR5 and its task scope (`964be64d` → `8ddefa0c`). The SPEC
resolves this with an explicit, scoped exception: structural assertions against
source text are permitted **only** for the two call connections FR1/FR2/FR3
name. Widening the scan to any third connection, or refactoring production for
observability, is outside the exception. Affected: task0001.

### D3 — Token-level scanning, never raw substring matching

The defect being closed is precisely a raw whole-file substring needle that one
unrelated call site keeps satisfied (A2). The replacement therefore compares
**token sequences inside an extracted function body**: comments and literal
interiors contribute nothing, and `apply_outcome` versus `apply_outcome_with_held`
are distinct identifiers rather than one being a prefix of the other (FR3, FR5,
FR6). Affected: task0001.

### D4 — Production freeze and change-set containment

`pointer_routing.rs`, `mouse_report.rs` and `event_loop.rs` stay at the base
revision. The observed change set for this feature must be contained in
`src-tauri/src/window_host/tests.rs` plus the workflow-generated
`feature-docs/` and `test-docs/` paths (FR4, AC-7). Affected: task0001.

### D5 — Zero new dependencies

No property-testing, parsing or benchmarking crate is introduced; the scanner is
hand-written against the standard library (NFR2). This is also what keeps the
licence ledger for the feature empty. Affected: task0001.

### D6 — Mutation inputs derive from the real source; the receiver identifier must not be re-bound

Two refinements adopted after a second-opinion review of this plan (recorded in
`phase-state/batch-audit.yaml`):

1. **TS-5's mutated inputs are rewrites of the real extracted bodies**, produced
   in memory from the compile-time embedded source — not hand-written bodies
   that resemble them. Each mutation asserts the unmutated body passes, the
   rewrite touched exactly one site, and the mutated body is rejected. A
   hand-written input can prove the judgment function correct while the body
   extractor's wiring to the real file is wrong; in that state the file-backed
   tests pass vacuously and the regression this feature targets still slips
   through (FR7).
2. **The held receiver identifier must still denote the parameter at the call
   site.** Membership in the enclosing function's parameter-identifier set is
   necessary but not sufficient: a same-named local binding introduced before
   the call satisfies the three-token shape while no longer referring to the
   live parameter. The judgment function therefore rejects any `let`, closure
   parameter, or `match` / `if let` / `while let` pattern that re-binds that
   identifier ahead of the call. The check is conservative and syntactic — no
   full Rust name resolution — and a conservative rejection is a red, never a
   silent pass (FR1, FR2, NFR8).

Affected: task0001.

## Risk Assessment

| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|------------|
| Scanner false green: a mis-handled comment or literal makes the scan match nothing, and an assertion passes vacuously | Medium | High — the feature would ship the very blind spot it exists to remove | "Not found" is an explicit failure, never a pass; TS-4 proves a correct call written inside a comment or a string literal does **not** satisfy the scan; TS-5 proves each mutation is rejected |
| Scanner false red on benign edits (re-wrapping, added comments, trailing comma) | Medium | Medium — the tests become an obstacle and get weakened | Scan is token-based with no dependence on line numbers, comment text, or the position of the next function; TS-4 covers the benign-edit inputs explicitly |
| Name coupling: a legitimate rename of a pinned name turns the suite red | Medium | Low | Documented trade-off (NFR8, SPEC *Known Trade-off*); the failure message states the rename duty so the fix is one line in the same change |
| Scope creep into production files while chasing a cleaner seam | Low | High — this is exactly what was reverted last time | FR4 freeze is restated in the task's Out of Scope; AC-7 verifies the change set mechanically |
| Synthetic mutation inputs drift from the real function shape, so the judgment function is correct but the extractor wiring is not — the file-backed tests then pass vacuously | Medium | High — the feature ships a blind spot while appearing covered | D6.1: TS-5 mutates the real extracted bodies and asserts unmutated-pass / exactly-one-site-changed / mutated-fail |
| Receiver identifier shadowed by a same-named local, so the three-token check passes while the live parameter is no longer what is passed | Low | High — a false green on the exact contract being pinned | D6.2: conservative re-binding rejection in the judgment function; a shadowing negative case in TS-5 and TS-4 |
| New tests duplicate or contradict the existing structural test | Low | Low | The new tests sit beside `pointer_routing_handlers_hold_no_decision_only_delegate_to_the_seam`; that test and its whole-file needle stay unmodified (FR8) |

## Open Questions

- [ ] None blocking. FR8 and NFR1–NFR7 carry no dedicated TS scenario in the
      SPEC's traceability table; they are constraint-type requirements verified
      through the AC-level checks recorded in `VERIFICATION.md` (suite run,
      feature-gate check, change-set inspection) rather than through a scenario
      of their own. Their `tests` arrays are intentionally left as the SPEC
      declares them. A second-opinion review agreed with keeping them empty
      rather than minting synthetic TS IDs that would duplicate the same checks
      under new names and diverge from the SPEC's traceability table
      (`phase-state/batch-audit.yaml`).
