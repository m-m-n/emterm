# Feature: focus-loss-drag-cleanup-test

## Overview

The focus-loss local-drag cleanup path introduced by
mouse-report-reset-active-gesture task0001 can currently be confirmed only by
source-scan assertions and manual checks, because it lives behind code that
assumes a winit window. This feature extracts a window-free state core, a pure
destination predicate and an injectable selection-write sink, then adds bare
`#[test]`s that drive the cleanup composition end-to-end without a window. It
changes no production behaviour; it adds test reachability and a documentation
backfill for the prior feature.

Requirements document: `feature-docs/focus-loss-drag-cleanup-test/REQUIREMENTS.md`.

## Objectives

- Make the focus-loss local-drag cleanup path verifiable by automated tests that
  run without a winit window, so the behaviour introduced by
  mouse-report-reset-active-gesture task0001 stops depending on source-scan
  assertions and manual confirmation alone (BO-1).
- Leave production behaviour byte-for-byte unchanged: this feature adds test
  reachability and nothing else (BO-2).
- Bring the prior feature's SPEC.md / VERIFICATION.md test-mapping rows back into
  agreement with reality once the new tests exist, without claiming unit
  verification for an effect only real hardware can confirm (BO-3).

## User Stories

### US1: Verify focus-loss cleanup without a window
As a developer of this repository, I want the focus-loss local-drag cleanup path
to be driven by tests that need no winit window, so that its three effects are
asserted by `cargo test --lib` rather than by source scanning.

**Acceptance Criteria:**
- [ ] AC-1: A bare `#[test]` drives the focus-loss cleanup composition to
      completion with no winit window and asserts the drag flag cleared, the
      pending anchor consumed, and the PRIMARY write recorded with the expected
      text.
- [ ] AC-2: All eight (selection_present, copy_on_select, text_empty)
      combinations of the destination predicate are asserted by bare `#[test]`s,
      and the asserted destination set matches FR6's rule in every one of them.
- [ ] AC-3: The recording sink observes both destination and payload, so a test
      can distinguish "PRIMARY only" from "PRIMARY and CLIPBOARD" and can assert
      the exact text written to each.

### US2: Refactor without disturbing shipped behaviour
As a developer of this repository, I want the test seam introduced without any
change to the shipped binary, so that the existing suite — including its
source-scan tests — keeps passing unmodified.

**Acceptance Criteria:**
- [ ] AC-4: `publish_local_drag`'s name, visibility, parameter order, parameter
      types and return type are unchanged, and the literals
      `local_drag_in_flight(host, &self.app)` and
      `publish_local_drag(host, &mut self.app)` still appear verbatim in
      event_loop.rs, so tests.rs:1932 passes without edit.
- [ ] AC-5: The full Rust suite passes unmodified apart from additions
      (`CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml`),
      with no existing test file's assertions weakened or deleted, and the
      CLI-only feature check still compiles.

### US3: Correct the prior feature's test mapping
As a developer of this repository, I want the prior feature's test-mapping rows
corrected once the new tests exist, so that the documentation states what is
actually verified and what still requires hardware.

**Acceptance Criteria:**
- [ ] AC-6: After the tests pass, mouse-report-reset-active-gesture's SPEC.md
      TS-4 checkbox and VERIFICATION.md TS-4 / SC-C rows point at the new tests,
      while that feature's AC-3 real-hardware confirmation remains mapped to a
      manual scenario and is not presented as unit-verified.

## Technical Requirements

### Functional Requirements

- **FR1 — Window-free drag-termination state core:** Extract the state half of
  the local-drag terminator into a window-free function in
  `src-tauri/src/window_host/pointer_routing.rs` that operates on plain inputs
  and owned state (the drag flag, the pending selection anchor, the resolved
  selection text, and `copy_on_select`) and never names `WindowHost`. It performs
  the two state effects `publish_local_drag` performs today — clearing the drag
  flag and consuming (taking) the pending selection anchor — and returns both the
  consumed anchor (`Option<Pos>`) and the destination decision of FR3. It is
  callable from a bare `#[test]` with no winit window, no GPU surface and no
  event loop.
- **FR2 — publish_local_drag delegates while keeping its spelling:**
  `publish_local_drag` (pointer_routing.rs:391) keeps its exact name, its
  `pub(super)` visibility, its parameter order and types
  `(host: &mut WindowHost, app: &mut App)`, and its `-> Option<Pos>` return, and
  becomes a thin adapter: it gathers the inputs, calls FR1's core, routes the
  resulting writes through FR4's sink, and returns the core's consumed anchor.
  The call-site literal `publish_local_drag(host, &mut self.app)` at
  event_loop.rs:254 and the literal `local_drag_in_flight(host, &self.app)` at
  event_loop.rs:253 are preserved character-for-character, because
  `focus_loss_arm_never_calls_the_fold_click_toggle` (tests.rs:1932) asserts both
  via `include_str!`.
- **FR3 — Pure destination predicate with full combination coverage:** Extract a
  pure predicate that maps the triple (selection_present, copy_on_select,
  text_empty) to the set of destinations to write — PRIMARY, CLIPBOARD, both, or
  neither. It takes plain values, returns a value, mutates nothing and performs
  no I/O. All eight input combinations are covered by bare `#[test]`s in
  `src-tauri/src/window_host/tests.rs`, in the style of the existing
  `drag_in_flight_is_true_whenever_either_input_is_true` (tests.rs:1916)
  truth-table test.
- **FR4 — Injectable sink seam plus recording test double:** Introduce a small
  injectable sink abstraction for the two selection writes (PRIMARY and
  CLIPBOARD) so the publish path writes through the seam rather than calling
  `host.set_primary` / `host.set_clipboard` directly. `WindowHost` provides the
  production implementation, preserving today's behaviour exactly. A recording
  test double in the test module captures each call's destination and payload, so
  a test with no winit window can assert both which destinations were written and
  the exact text written to each. SCOPE BOUND: the seam applies only to
  pointer_routing.rs's publish path. key_routing.rs:106's
  `host.set_clipboard(&text);` is left untouched —
  `copy_chord_clears_selection_immediately_after_clipboard_write` (tests.rs:2474)
  source-scans key_routing.rs for that exact literal and for its adjacency to
  `app.clear_selection();`.
- **FR5 — Window-free end-to-end focus-loss cleanup test:** Add a bare `#[test]`
  (no winit window) that drives the focus-loss cleanup composition end-to-end —
  the drag-in-flight guard followed by the terminator's publish half — against
  the FR1 core and the FR4 recording sink, asserting TS-4's three effects: (a)
  the drag flag is cleared, (b) the pending selection anchor is consumed and
  returned, (c) the selection text is published to PRIMARY (and to CLIPBOARD
  exactly when `copy_on_select` is on and the text is non-empty), with the
  recorded payload matching the resolved selection text.
- **FR6 — Existing write rule preserved byte-for-byte:** The write rule in force
  today is preserved exactly, with no repair or tightening: PRIMARY is written
  for ANY resolved selection including empty text; CLIPBOARD is written only when
  `app.settings.copy_on_select` is true AND the resolved text is non-empty;
  neither destination is written when `app.selection` is `None` or the active tab
  lookup fails. Any perceived defect in this rule (for example the empty-text
  PRIMARY write) is out of scope and is pinned by the new tests as current
  behaviour rather than changed.
- **FR7 — Fold-toggle exclusivity preserved:** The fold-click toggle stays
  outside the terminator's publish half and outside FR1's core:
  `handle_fold_click` remains exclusive to
  `complete_selection_and_publish_to_primary`'s release-path composition in
  pointer_routing.rs (line 424ff) and the string `handle_fold_click` must never
  appear in event_loop.rs. `focus_loss_arm_never_calls_the_fold_click_toggle`
  (tests.rs:1932) continues to pass unmodified.
- **FR8 — Prior-feature document backfill:** Once the new tests land and pass,
  correct `feature-docs/mouse-report-reset-active-gesture/SPEC.md`'s TS-4
  checkbox and `feature-docs/mouse-report-reset-active-gesture/VERIFICATION.md`'s
  TS-4 and SC-C rows so they point at the new tests. The backfill must record
  that a recorded sink call is evidence of the write REQUEST, not of an actual
  OS-level PRIMARY/CLIPBOARD update, and therefore must keep that prior feature's
  AC-3 real-hardware confirmation mapped to a manual scenario rather than marking
  it unit-verified.

### Non-Functional Requirements

- **NFR1 - Window-free reachability:** Every new test added by this feature runs
  under
  `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib`
  with no winit window, no GPU surface, no event loop and no display server. No
  new test may depend on constructing a `WindowHost`.
- **NFR2 - decide_\* purity:** The decision functions (FR1's core decision and
  FR3's destination predicate) are pure: they take values, return values, perform
  no I/O, touch no globals and mutate no caller-visible state beyond the owned
  state they are handed. All side-effecting work stays on the adapter side (FR2)
  or behind the sink (FR4).
- **NFR3 - No production behaviour change:** The refactor is observationally
  neutral in the shipped binary: same destinations, same payloads, same ordering,
  same state mutations, same return value, for every input. The whole existing
  suite passes unmodified, including the three source-scan tests at tests.rs:1932,
  tests.rs:2448 and tests.rs:2474.
- **NFR4 - Identifier-collision avoidance:** `drag_in_flight` is already an
  overloaded identifier in this tree — a free function (pointer_routing.rs:364),
  a `ButtonEventInputs` field (mouse_report.rs:629) and local test bindings
  (tests.rs:1889, 3345, 3863, 3969, 4054). New names introduced by FR1/FR3/FR4
  must not add a fourth meaning to it or shadow the existing ones.

## Implementation Approach

### Architecture

**Component layering (after the refactor):**

```
event_loop.rs  (focus-loss arm; call-site literals preserved verbatim)
        │  local_drag_in_flight(host, &self.app)
        │  publish_local_drag(host, &mut self.app)
        ▼
pointer_routing.rs :: publish_local_drag        ← adapter (FR2, side effects)
        │  gathers inputs from host/app
        ▼
pointer_routing.rs :: window-free state core    ← pure-ish core (FR1, NFR2)
        │  clears drag flag, takes pending anchor
        ▼
pointer_routing.rs :: destination predicate     ← pure (FR3, NFR2)
        │  (selection_present, copy_on_select, text_empty) → destination set
        ▼
selection-write sink (FR4)
        ├── production impl on WindowHost  → host.set_primary / host.set_clipboard
        └── recording test double          → records (destination, payload)
```

The fold-click toggle is deliberately absent from this chain: `handle_fold_click`
belongs only to `complete_selection_and_publish_to_primary`'s release-path
composition (FR7).

**Component Diagram:**

```
Adapter (FR2)  — owns all side effects; keeps its exact public spelling
Core (FR1)     — owns the two state effects; returns consumed anchor + decision
Predicate (FR3)— owns the destination decision only
Sink (FR4)     — owns the two selection writes; two implementations
Tests          — drive Core + Predicate + recording Sink with no WindowHost
```

### Data Flow

```
focus loss → guard (local_drag_in_flight) → adapter (publish_local_drag)
           → core: clear drag flag, take pending anchor
           → predicate: decide destination set
           → sink: write PRIMARY / CLIPBOARD as decided
           → adapter returns Option<Pos> (the consumed anchor)
```

### Write Rule (FR6, preserved)

| selection_present | copy_on_select | text_empty | destinations written |
|---|---|---|---|
| false | false | — | none |
| false | true | — | none |
| true | false | false | PRIMARY |
| true | false | true | PRIMARY |
| true | true | false | PRIMARY + CLIPBOARD |
| true | true | true | PRIMARY |

Neither destination is written when `app.selection` is `None` or the active tab
lookup fails. The empty-text PRIMARY write is preserved as-is, not repaired.

### API Design

Not applicable: this feature adds no network, IPC or control-sequence surface.

### Database Schema

Not applicable: this feature persists no data.

### Dependencies

**Internal Dependencies:**
- `src-tauri/src/window_host/pointer_routing.rs`: hosts the terminator, the new
  core (FR1), the predicate (FR3) and the sink call site (FR4).
- `src-tauri/src/window_host/event_loop.rs`: the focus-loss arm whose two
  call-site literals are pinned by tests.rs:1932 (FR2, FR7).
- `src-tauri/src/window_host/mod.rs`: defines `host.set_primary` (:705) and
  `host.set_clipboard` (:731); hosts the production sink implementation.
- `src-tauri/src/window_host/tests.rs`: hosts the new bare `#[test]`s and the
  recording test double, and already carries the three source-scan tests
  (:1932, :2448, :2474).
- `src-tauri/src/window_host/key_routing.rs`: out of scope for the seam; its
  `host.set_clipboard(&text);` literal at :106 stays as it is.
- `feature-docs/mouse-report-reset-active-gesture/`: the prior feature whose
  SPEC.md and VERIFICATION.md rows FR8 corrects.

**External Dependencies:**
- None added by this feature.

### Constraints and Risks

- No format command is documented in any project rules file — `make fmt` is
  history-sourced, and the project forbids crate-wide `cargo fmt`, so formatting
  must stay scoped to the touched files.
- `host.set_primary` / `host.set_clipboard` are defined at
  `src-tauri/src/window_host/mod.rs:705` and `:731`, which was not fully scanned,
  so the seam's production implementation lands in a file whose reference impact
  is only partially known.
- `tests.rs:2474` scans `key_routing.rs` for the literal
  `host.set_clipboard(&text);` with an adjacency requirement, which is what bounds
  the seam to `pointer_routing.rs`.
- A recorded sink call proves the write REQUEST, not an OS-level update; TS-8
  stays manual for that reason, and FR8's backfill must say so.
- `drag_in_flight` is an overloaded identifier; new names must not collide with
  or shadow it (NFR4).
- Unit tests live in inline `#[cfg(test)] mod tests` and run under `--lib`; the
  project's own note records that `--bin emterm` yields zero tests.
- There is no E2E infrastructure in this repository, so no E2E scenario is added
  and the E2E run command is empty.

### File Structure

```
src-tauri/src/window_host/
├── pointer_routing.rs   # terminator adapter (FR2) + core (FR1) + predicate (FR3)
├── event_loop.rs        # focus-loss arm; call-site literals preserved (FR2, FR7)
├── mod.rs               # production sink impl on WindowHost (FR4)
├── tests.rs             # new bare #[test]s + recording sink double (FR3, FR5)
└── key_routing.rs       # untouched (seam scope bound, FR4)

feature-docs/mouse-report-reset-active-gesture/
├── SPEC.md              # TS-4 checkbox backfill (FR8)
└── VERIFICATION.md      # TS-4 / SC-C row backfill (FR8)
```

## Declared Change Set

This section states the create-plan derivation instead of a hand-authored list:
the feature-specific paths are derived at create-plan from every task's `files`
entries in `workflow.yaml` (`references/phases/create-plan-phase.md`).

Every SPEC declares, by default, the following two workflow-generated entries in
addition to the feature-specific paths:

- `feature-docs/{feature}/**`
- `test-docs/{feature}/**`

`feature-docs/{feature}/**` covers `REQUIREMENTS.md`, `SPEC.md`,
`IMPLEMENTATION.md`, `workflow.yaml`, `phase-state/`, `tasks/`,
`reviews/roundN.yaml`, `VERIFICATION.md`, `retrospect.yaml`, and the design
artifacts the design step produces. These are generated and owned by the phase
documents and by `references/phase-state.md`; this section cites them and
restates none of their rules.

`test-docs/{feature}/**` covers `test-docs/{feature}/{T}.tests.yaml`, the
per-task test record. It is generated and owned by `implement-phase.md`; this
section cites it and restates none of its rules.

These two default entries are part of the declaration unless the SPEC author
explicitly removes them; their absence is never assumed by silence — removal is
a deliberate, explicit narrowing.

This declaration is a SUPERSET assertion: the actual change set observed at
verification time must be CONTAINED IN the declared set, not equal to it.

The analysis's declared change set for this feature is:

- `src-tauri/src/window_host/pointer_routing.rs`
- `src-tauri/src/window_host/event_loop.rs`
- `src-tauri/src/window_host/mod.rs`
- `src-tauri/src/window_host/tests.rs`
- `feature-docs/mouse-report-reset-active-gesture/SPEC.md`
- `feature-docs/mouse-report-reset-active-gesture/VERIFICATION.md`

## Test Scenarios

### Unit Tests
- [ ] TS-1 (FR3, FR6, NFR1, NFR2): Destination predicate truth table — all eight
      combinations of (selection_present, copy_on_select, text_empty) assert the
      expected destination set: no selection writes nothing; selection present
      always writes PRIMARY including empty text; CLIPBOARD only on
      copy_on_select AND non-empty text.
- [ ] TS-2 (FR1, NFR1, NFR2): State core effects — with a drag flag set and a
      pending anchor present, the core clears the flag and returns the consumed
      anchor; with neither set it returns `None` and leaves state clean.
- [ ] TS-3 (FR4, FR6, NFR1): Recording sink payloads — publishing a non-empty
      selection with `copy_on_select` on records exactly two calls (PRIMARY then
      CLIPBOARD) carrying identical text; with `copy_on_select` off it records
      exactly one PRIMARY call.
- [ ] TS-4 (FR1, FR4, FR5, NFR1): Focus-loss cleanup end-to-end without a winit
      window — guard true, publish half runs, and the three effects (flag
      cleared, anchor consumed, selection published) are all asserted in one
      test. This is the scenario the prior feature could only confirm by source
      scan.

### Integration Tests
- None. The feature's verification is entirely at the unit, regression, build and
  manual levels listed here.

### E2E Tests
**Existing E2E tests**: None — this repository has no E2E infrastructure.
**Run command**: Not detected.

### Edge Cases
- [ ] TS-5 (FR3, FR6, NFR1): Empty-selection edge — a resolved selection whose
      text is empty still records a PRIMARY call and records no CLIPBOARD call
      even with `copy_on_select` on (preserved behaviour, FR6).

### Regression Tests
- [ ] TS-6 (FR2, FR7, NFR3, NFR4): The three existing source-scan tests
      (tests.rs:1932 fold-toggle exclusion, tests.rs:2448 forwarded-branch clear,
      tests.rs:2474 copy-chord clipboard adjacency) pass unmodified.

### Build Checks
- [ ] TS-7 (FR4, NFR3):
      `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml --no-default-features`
      still succeeds, confirming no GUI-only type leaked into CLI-shared code.

### Manual Scenarios
- [ ] TS-8 (FR8): Real-hardware PRIMARY/CLIPBOARD confirmation for the prior
      feature's AC-3 stays manual: select text, remove focus, middle-click paste
      elsewhere. A recorded sink call cannot substitute for this.

### Performance Tests
- Not applicable: this feature adds no runtime work to the shipped binary.

## Security Considerations

- **Authentication / Authorization:** Not applicable — no authenticated surface
  is involved.
- **Input Validation:** Not applicable — the new functions take in-process values
  only.
- **Data Protection:** The selection text handled by the sink is the same data
  the current publish path handles; the seam changes where the write call goes,
  not what data is written (NFR3).
- **XSS / SQL Injection / CSRF:** Not applicable — no web surface and no database
  is involved.

## Error Handling

The publish path's only failure-shaped conditions are the two no-write cases FR6
pins: `app.selection` is `None`, and the active tab lookup fails. Both result in
no write to either destination and no error surfaced to the user. This feature
introduces no new error paths and no new error codes.

```
no selection OR tab lookup fails → write nothing → return None
```

## Performance Optimization

Not applicable. NFR3 requires the shipped binary to be observationally neutral,
so no performance goal, optimization strategy or caching behaviour changes.

## Success Criteria

- [ ] All functional requirements FR1–FR8 are implemented and tested.
- [ ] TS-1 through TS-7 pass; TS-8 is executed manually and recorded as a manual
      scenario.
- [ ] AC-1 through AC-6 are satisfied.
- [ ] Production behaviour is unchanged (NFR3): the full existing suite passes
      unmodified, including tests.rs:1932, tests.rs:2448 and tests.rs:2474.
- [ ] Every new test runs window-free under `--lib` (NFR1) and no new test
      constructs a `WindowHost`.
- [ ] The CLI-only feature check still compiles (TS-7).
- [ ] The prior feature's documents are backfilled and its AC-3 remains mapped to
      a manual scenario (FR8, AC-6).
- [ ] Formatting stays scoped to the touched files.

## Open Questions

> **Note**: 未解決の要件は workflow.yaml で `status: tbd` として管理されています。
> plan フェーズの実行前に解決してください。

None. All of FR1–FR8 are `status: resolved`.

## Implementation Phases (if applicable)

### Phase 1: Test seam extraction
**Goals:** Make the cleanup path reachable from window-free tests without
changing behaviour.
**Deliverables:**
- FR1's window-free state core in pointer_routing.rs
- FR3's pure destination predicate
- FR4's sink abstraction with the production implementation on `WindowHost`
- FR2's adapter form of `publish_local_drag`, with its spelling and call-site
  literals preserved

### Phase 2: Tests
**Goals:** Assert the behaviour the prior feature could only source-scan.
**Deliverables:**
- TS-1's eight-combination truth-table tests
- TS-2's state-core tests
- TS-3's recording-sink payload tests
- TS-4's end-to-end window-free cleanup test
- TS-5's empty-selection edge test
- TS-6 / TS-7 confirmation runs

### Phase 3: Prior-feature backfill
**Goals:** Bring the prior feature's test mapping into agreement with reality.
**Deliverables:**
- `feature-docs/mouse-report-reset-active-gesture/SPEC.md` TS-4 checkbox update
- `feature-docs/mouse-report-reset-active-gesture/VERIFICATION.md` TS-4 and SC-C
  row updates, recording that a sink call is evidence of the write REQUEST only
  and keeping AC-3 mapped to a manual scenario

## References

- Requirements document: `feature-docs/focus-loss-drag-cleanup-test/REQUIREMENTS.md`
- Prior feature spec: `feature-docs/mouse-report-reset-active-gesture/SPEC.md`
- Prior feature verification: `feature-docs/mouse-report-reset-active-gesture/VERIFICATION.md`
- Terminator and publish path: `src-tauri/src/window_host/pointer_routing.rs`
- Focus-loss arm: `src-tauri/src/window_host/event_loop.rs`
- Selection write primitives: `src-tauri/src/window_host/mod.rs:705`, `:731`
- Existing tests and source scans: `src-tauri/src/window_host/tests.rs`
- Project commands:
  - check: `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml`
  - test: `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml`
  - CLI-only check: `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml --no-default-features`
  - format: `make fmt` (history-sourced; keep scoped to touched files)
  - build: `CARGO_TARGET_DIR=src-tauri/target-host cargo build --release --manifest-path src-tauri/Cargo.toml`
- License: MIT
