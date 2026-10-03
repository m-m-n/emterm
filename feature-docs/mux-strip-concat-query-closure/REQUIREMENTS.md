---
title: "mux-strip-concat-query-closure"
created_date: 2026-10-03
status: draft
---

# mux-strip-concat-query-closure - Requirements

## 1. Overview

### 1.1 Background

When the shared strip removes a construct together with its opening ESC, the
bytes before and after it are joined. The three FR6 residuals recorded in
`feature-docs/mux-cut-csi-post-strip-closure/DECISIONS.md`, and the same
mechanism after a superseded lone ESC, remain open.

### 1.2 Purpose

- The join produced by the shared strip never completes an escape or CSI
  device query the raw stream never made, so a replayed snapshot or live
  continuation never makes the client send an unsolicited reply
  (`ESC[row;colR` and similar) to the PTY.
- Close the three FR6 residuals recorded in
  `feature-docs/mux-cut-csi-post-strip-closure/DECISIONS.md`, plus the same
  mechanism after a superseded lone ESC.
- Add regression tests that detect each case.

### 1.3 Scope

- In scope: daemon-side byte-stream filtering (scrollback_filter /
  write_filter / snapshot assembly): every shared-strip entry point, the
  write filter's carried CSI classification, snapshot layouts, offset
  remapping, and the records listed in F08.
- Out of scope: rings captured by a pre-fix daemon and carried over by
  hot-upgrade (EC-10).
- Design step: skipped (no UI or visual surface).

## 2. Business Requirements

### 2.1 Business Objectives

- When the shared strip removes a construct together with its opening ESC,
  the bytes before and after it are joined. That join must never complete an
  escape or CSI device query the raw stream never made, so a replayed
  snapshot or live continuation never makes the client send an unsolicited
  reply (`ESC[row;colR` and similar) to the PTY.
- Close the three FR6 residuals recorded in
  `feature-docs/mux-cut-csi-post-strip-closure/DECISIONS.md`, plus the same
  mechanism after a superseded lone ESC.
- Add regression tests that detect each case.

### 2.2 Target Users

| User type | Description |
|-----------|-------------|
| mux user | A user whose shell or TUI runs inside an eMterm mux session and who switches tabs, reattaches or resumes visibility. |

### 2.3 Expected Effects

- After a tab switch, reattach or visibility resume, the shell or TUI never
  receives an unsolicited cursor-position or device-attributes reply
  (`ESC[row;colR` and similar) that the raw output did not request.

## 3. Use Cases

### 3.1 Use Case List

| ID | Use case | Actor | Priority |
|----|----------|-------|----------|
| UC01 | Tab switch, reattach or visibility resume replays a snapshot | mux user | High |

### 3.2 Use Case Details

#### UC01: Tab switch, reattach or visibility resume replays a snapshot

**Actor**: mux user

**Preconditions**:
- PTY output written to the ring contains a construct the shared strip
  removes (OSC 777 viewer launch, OSC 9999 emterm-md, agent-status report,
  Kitty APC, SIXEL DCS or an answered CSI device query) inside an open CSI or
  after a lone ESC, or contains a CSI device query split across reads.

**Main flow**:
1. The user switches tabs, reattaches or resumes visibility.
2. The client replays the snapshot, then receives live continuation output.

**Postconditions**:
- The shell or TUI receives no unsolicited cursor-position or
  device-attributes reply that the raw output did not request.

## 4. Functional Requirements

### 4.1 Function List

| ID | Function | Description | Priority |
|----|----------|-------------|----------|
| FR1 | Strip closes an open CSI at a removed construct | Every shared-strip entry point writes CSI_CLOSING at the removal position when its written stream is inside a CSI. | High |
| FR2 | No snapshot leaves a strip-induced open CSI | A ring written from `ESC[6` + a removed construct no longer ends inside a CSI. | High |
| FR3 | Split CSI device query never reaches the ring answerable | The write filter carries an O(1) classification of the open CSI and writes CSI_CLOSING in place of a completing query byte. | High |
| FR4 | Neutralize a superseded lone ESC before a removed construct | The strip neutralizes a superseded lone ESC at the removal position. | High |
| FR5 | Path identity and predicate stability | Every strip entry point produces byte-identical output for the same input and start state. | High |
| FR6 | Offset remapping stays consistent | Remapping counts inserted closing or neutralizing bytes. | High |
| FR7 | Cut and carried-state behavior preserved | The predecessor's cut rules still hold. | High |
| FR8 | Records | DECISIONS.md and test-docs records reflect changed and renamed tests. | Medium |

### 4.2 Function Details

#### FR1: Strip closes an open CSI at a removed construct

**Description**: Every shared-strip entry point (`strip_replayable_rich_content`,
`strip_pty_output_for_scrollback_write`,
`strip_rich_content_and_remap(_with_designator)`, the designator form and the
state-reporting form) writes CSI_CLOSING (DEL) at the position of a construct
it removes together with its opening ESC whenever its written stream is inside
a CSI there (entry or parameter sub-state). This covers OSC 777 viewer
launches, OSC 9999 emterm-md, agent-status reports, Kitty APC, SIXEL DCS and
answered CSI device queries.

**Business rules**:
- C0 bytes re-emitted from a removed query are still written once, in order.
- In ground no closing is written.
- After the closing the written stream is in ground, so further removed
  constructs in the same run need no second closing.
- Example: `ESC[6` + launch + `n` in one cut-free call yields ring bytes whose
  replay shows `n` as text and produces no response.

#### FR2: No snapshot leaves a strip-induced open CSI

**Description**: Because the strip writes the closing at the removal position
(FR1, write path and snapshot path), a ring written from `ESC[6` + a removed
construct no longer ends inside a CSI. A visibility-resume or reattach
snapshot of it, followed by a live `n`, produces no response, and the client
matches the raw-stream reference.

**Business rules**:
- The snapshot layout adds no closing at its end: a CSI the app genuinely
  left open stays open.
- The reattach trailing `ESC[?1049{h,l}`, the resume layouts and the
  wrapped-ring dump block are unchanged.
- Rings captured by a pre-fix daemon and carried over by hot-upgrade are out
  of scope.

#### FR3: Split CSI device query never reaches the ring answerable

**Description**: The write filter does not hold CSI bytes. Across calls it
carries an O(1) classification of the open CSI in its written stream: the CSI
sub-state, private marker, first-parameter accumulation and intermediates,
following term_core's transitions (`csi_step`) and response conditions
(`csi_is_device_query`, including the clamp and u8 truncation of the `n` row
and the intermediate-slot rules). When a later call's bytes complete that CSI
as an answered device query, the filter writes CSI_CLOSING in place of the
completing final byte.

**Business rules**:
- C0 bytes arriving inside the continuation are written once, in order.
- The ring then holds no executable query: a remaining `ESC[6` + DEL is
  allowed.
- A split CSI that completes as a non-query (for example SGR) is written
  unchanged.
- Written bytes do not have to match between split and whole-call input;
  replay equivalence is required.
- At a cut the classification is cleared along with the CSI state.

#### FR4: Neutralize a superseded lone ESC before a removed construct

**Description**: When the strip has written a lone ESC (an ESC that the next
ESC supersedes, written by the fallback arm) and the construct opened by that
next ESC is removed, the strip neutralizes the lone ESC at the removal
position so that the bytes after the construct cannot form an escape or a
query with it. Examples: `ESC` + `ESC[6n` + `[c` and `ESC` + launch + `[6n`.

**Business rules**:
- After neutralization the written stream is in ground, so the carried state
  (CSI-only, `csi()` returns None for Escape) never has to represent an
  Escape state across calls.
- This applies both within one call and across calls (the write filter holds
  a lone trailing ESC and an ESC ESC chain until it settles, so the join is
  always seen inside one strip pass).
- The neutralizing bytes, or the omission of the superseded ESC, must have no
  display, cursor or response effect in term_core and are counted in offset
  remapping.

#### FR5: Path identity and predicate stability

**Description**: For the same input and start state, every strip entry point
produces byte-identical output, so the write and snapshot paths stay
identical (D1').

**Business rules**:
- The state-reporting form started from an open CSI may differ from a
  ground-started form only by the closing bytes FR1/FR3 require.
- The strip-target predicates (`scan_csi_device_query`,
  `csi_is_device_query`, `is_replayable_osc_body`, `dcs_is_sixel`) and the
  set of kept bytes are unchanged.

#### FR6: Offset remapping stays consistent

**Description**: `strip_rich_content_and_remap(_with_designator)` counts
inserted closing or neutralizing bytes in the output.

**Business rules**:
- Remapped watch offsets stay non-decreasing and within the output for
  offsets at the start of, strictly inside, and right after a removed span.
- `snapshot_bytes` segment offsets keep pointing at the same content.

#### FR7: Cut and carried-state behavior preserved

**Description**: The predecessor's cut rules still hold.

**Business rules**:
- At most one closing per cut, never together with the designator ESC.
- The empty-segment, fallback and overflow-flush paths close an open CSI.
- After every cut-free call the carried CSI state equals term_core's on the
  written bytes.
- A cut that follows a strip-written closing writes no second closing.

#### FR8: Records

**Description**: This feature's DECISIONS.md lists every existing test whose
expectation changes under Behavior-changing tests.

**Business rules**:
- Renamed tests listed in predecessor test-docs records are updated per
  `.claude/rules/test-docs-records.md` and resolve in the `--lib` test
  listing.
- The predecessor's DECISIONS.md is not edited.

## 5. Non-Functional Requirements

### 5.1 Performance Requirements

- NFR1: The strip remains a single O(n) pass with O(1) extra state. The write
  filter's carried CSI classification is O(1). No CSI byte is held in
  pending, and `pending()`'s postcondition is unchanged.
- NFR2: Adversarial inputs (long alternations of open CSIs and strip targets,
  long CSI parameter runs fed whole and byte by byte, the overflow path, long
  ESC ESC chains before removed constructs) finish within the existing 10 s
  test budgets and never panic.

### 5.2 Security Requirements

- PTY output is attacker-influenceable. The filter must stay bounded in time
  and memory for adversarial streams (NFR1, NFR2), and no byte sequence may
  make the client send an unsolicited reply to the PTY on replay.

### 5.3 Terminal Behavior Requirements

- NFR3: Closing and neutralizing bytes cause no display, cursor or response
  change in term_core.

### 5.4 Build and Test Requirements

- NFR4: Both
  `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib`
  and
  `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml --no-default-features`
  pass.

## 6. UI/UX Requirements

- After a tab switch, reattach or visibility resume, the shell or TUI never
  receives an unsolicited cursor-position or device-attributes reply
  (`ESC[row;colR` and similar) that the raw output did not request.
- No UI or visual surface changes; the design step is skipped.

## 7. Data Requirements

Not applicable.

## 8. External Integration

Not applicable.

## 9. Constraints

### 9.1 Technical Constraints

- The closing for an open CSI is the existing CSI_CLOSING (DEL) (A1).
- The strip-target predicates and the kept bytes are unchanged;
  `suppressed_output` keeps reusing `scan_csi_device_query` (A2).
- The regression oracle is term_core fed the raw stream, with the removed
  construct's own effect excluded (A3).

### 9.2 Business Constraints

- The predecessor's DECISIONS.md is not edited. Test-docs records are updated
  only for renamed tests, per `.claude/rules/test-docs-records.md` (A8).

### 9.3 Schedule Constraints

None.

### 9.4 Declared Change Set

Feature-specific paths are not enumerated by hand; they are derived at
create-plan from each task's `files` in `workflow.yaml`
(`references/phases/create-plan-phase.md`).

**Default members** (always part of the declaration unless the SPEC author
explicitly removes them):
- `feature-docs/mux-strip-concat-query-closure/**`
- `test-docs/mux-strip-concat-query-closure/**`

`feature-docs/mux-strip-concat-query-closure/**` covers `REQUIREMENTS.md`,
`SPEC.md`, `IMPLEMENTATION.md`, `workflow.yaml`, `phase-state/`, `tasks/`,
`reviews/roundN.yaml`, `VERIFICATION.md`, `retrospect.yaml`, and the design
artifacts the design step produces. Their generators are the phase documents
and `references/phase-state.md` (cited only; rules are not restated).

`test-docs/mux-strip-concat-query-closure/**` covers `{T}.tests.yaml` (path
form: `test-docs/mux-strip-concat-query-closure/{T}.tests.yaml`). Its
generator is `implement-phase.md` (cited only; rules are not restated).

**Semantics**:
- Default members are part of the declaration unless the SPEC author
  explicitly removes them. Removal is a deliberate narrowing, not an omission.
- The declaration is a superset assertion: the actual change set must be
  CONTAINED IN the declared set. A declared path that is never generated is
  not a violation. A feature that generates no implement task generates no
  `test-docs/mux-strip-concat-query-closure/` directory, and the declared
  `test-docs/mux-strip-concat-query-closure/**` is still correct.

## 10. Anticipated Issues and Risks

### 10.1 Technical Issues

| Issue | Impact | Response |
|-------|--------|----------|
| About 15 existing expectations change (BYTE_BY_BYTE_EXPECTATIONS, CLOSING_CASES, post_strip_* tests, strip_bare_esc_in_csi_body_aborts_then_strips_following_query, post_strip_state_form_* tests), including the `round4_cut_csi.rs:621` expectation and the inversion of `post_strip_cut_csi.rs` (b). | Medium | List each change in this feature's DECISIONS.md (FR8). |
| The state-reporting strip form's signature likely changes to carry the classification (FR3). | Medium | Path identity is verified by FR5. |
| EC-2 depends on the `suppressed_output` re-delivery, which was not inspected. | Medium | Verify with the reader harness (TS-3). |

### 10.2 Business Risks

None.

## 11. Success Criteria

### 11.1 Acceptance Criteria

- [ ] AC-1 (FR1): One cut-free call of each open head (`ESC[`, `ESC[6`,
  `ESC[?25`, `ESC[6 space`, `abc ESC[12;3`) + each removed construct kind +
  `n` (and `c` / `t`): replaying the written ring through term_core gives no
  response and the same rows and cursor as the raw-stream reference, with the
  construct's own effect excluded.
- [ ] AC-2 (FR2): Through the production reader and the visibility-restore
  harness, with no cut: `ESC[6` + construct in one read, the restore taken
  after that read, then `n` in a later read. The client's responses, rows and
  cursor equal the raw-stream reference's. The same holds for the reattach
  layout.
- [ ] AC-3 (FR3): For each answered query form (`ESC[6n`, `ESC[5n`,
  `ESC[261n`, `ESC[c`, `ESC[0c`, `ESC[?c`, `ESC[>c`, `ESC[14t`, `ESC[16t`,
  `ESC[18t`, `ESC[?1$p`), split at every position, fed byte by byte, and with
  C0 bytes inside the continuation, the ring holds no executable query and
  replaying the ring gives no response. C0 bytes keep their effect. Non-query
  CSIs split the same way are written unchanged. pending stays empty
  throughout.
- [ ] AC-4 (FR4): `ESC` + `ESC[6n` + `[c`, `ESC` + launch + `[6n`, and `ESC`
  + Kitty + `[c`, in one call and split at every position: replay gives no
  response and equals the raw-stream reference.
- [ ] AC-5 (FR5, FR6): The identity tests across strip entry points pass for
  equal start states. The remap tests pass, and new cases cover offsets at
  the start of, inside and right after a construct removed inside an open CSI
  and after a lone ESC. `snapshot_bytes` segment tests pass.
- [ ] AC-6 (FR7, FR8): The predecessor cut, carry and closing tests pass, with
  expectations updated only where FR1, FR3 or FR4 change written bytes. Each
  change is listed in DECISIONS.md, and any renamed test is updated in the
  test-docs records.
- [ ] AC-7 (NFR1, NFR2, NFR4): The linear-pass and adversarial-budget tests
  pass within the budget, and both the `--lib` tests and the
  `--no-default-features` check succeed.

### 11.2 KPI

Not applicable.

## 12. Test Scenarios

### 12.1 Test Perspectives

- [ ] TS-1 (AC-1): Strip table: open head x removed construct kind x
  continuation (`n`, `c`, `t`, `m`, text). Assert the written bytes, the
  reported state (None after the closing), and the term_core view equal to
  the raw-stream reference.
- [ ] TS-2 (AC-1): Write filter, one cut-free call `ESC[6` + target + `n`,
  for every target kind including an embedded-C0 query (EC-3) and several
  targets in one CSI (EC-4: exactly one closing).
- [ ] TS-3 (AC-2): Reader level: `run_visibility_restore_at` with chunks
  [`ESC[6` + target, `n`] and [`ESC[6` + target, CR, `n`], restore after the
  first read, no screen switch. Compare with
  `assert_client_equals_reference`-style helpers. Also cover the held-target
  variant (EC-2): target split across reads with the restore between them.
- [ ] TS-4 (AC-3): Write filter: each answered query form split at every
  position and fed byte by byte, then replayed. Assert no response, no
  executable query in the ring, and `pending_len() == 0` after every call.
- [ ] TS-5 (AC-3): Classification parity: for open CSI prefixes (private
  marker, intermediates, long or saturating parameters, `ESC[261`) and every
  completing byte, the filter's closing decision matches whether term_core
  answers the raw stream.
- [ ] TS-6 (AC-4): Strip and write filter: `ESC ESC` + removed construct +
  `[c` / `[6n`, in one call and split at every position (including a lone
  trailing ESC held across calls). Replay equals the raw-stream reference.
- [ ] TS-7 (AC-5): Remap: watch offsets at the start of, inside and right
  after a removed span inside an open CSI and after a lone ESC. Assert
  non-decreasing offsets within the output and stable `build_snapshot_bytes`
  segments.
- [ ] TS-8 (AC-6): Rerun the updated predecessor suites (`round4_cut_csi`,
  `post_strip_cut_csi`, `scrollback_filter` tests): cut closings, designator
  exclusivity, split invariance in replay terms, overflow flush.
- [ ] TS-9 (AC-7): Budget tests: alternations of open CSI + target (and lone
  ESC + target) at 100k repetitions, plus a 300k-byte CSI fed byte by byte,
  each within 10 s.

### 12.2 Edge Cases

- EC-1: The open CSI is in one call and the strip target is held and
  completed in a later call. The completing call's strip starts from the
  carried CSI and writes the closing, so the carried state becomes None. This
  inverts `post_strip_cut_csi.rs` (b).
- EC-2: A snapshot taken while the target after an open CSI is still held:
  the ring ends in `ESC[6` and pending holds the target head. Verify with the
  reader harness that no response results; this depends on the
  `suppressed_output` re-delivery, which was not inspected.
- EC-3: A removed query with an embedded C0 byte after an open CSI: the C0
  byte executes once and the CSI is closed.
- EC-4: Several removed constructs inside one open CSI get exactly one
  closing.
- EC-5: Entry sub-state: `ESC[` + launch + `c` must not form DA1.
- EC-6: The designator wait and a closing are never written at the same cut.
- EC-7: On the overflow flush path, the strip of the flushed run writes the
  closing in place.
- EC-8: With `ESC[6` in one call and `ESC[6n` in the next, the second call's
  strip removes the query from the carried CSI state and writes the closing.
- EC-9: A split non-query CSI (`ESC[3 | 1m`) is written unchanged. A split
  query with C0 bytes in between (`ESC[6 | CR | n`) keeps the C0 effect and
  writes the closing instead of `n`.
- EC-10: A ring captured by a pre-fix daemon and carried over by hot-upgrade
  is out of scope.
- EC-11: A lone ESC held across calls (`ESC | ESC[6n[c`) is resolved inside
  one strip pass after the chain settles.

## 13. Glossary

| Term | Definition |
|------|------------|
| CSI_CLOSING | The existing DEL closing byte written to end an open CSI. |
| Lone ESC | An ESC that the next ESC supersedes, written by the fallback arm. |
| Raw-stream reference | term_core fed the raw stream, with the removed construct's own effect excluded. |

## 14. Confirmed Items

### 14.1 Confirmed

- [x] A1: The closing for an open CSI is the existing CSI_CLOSING (DEL).
  Reason: predecessor tests pin its no-effect cancellation and that no strip
  reads it as a target. Impact: medium. Reversible: yes.
- [x] A2: The strip-target predicates and the kept bytes are unchanged;
  `suppressed_output` keeps reusing `scan_csi_device_query`. Reason: this is
  the predicate SSOT shared with `suppressed_output`. Impact: medium.
  Reversible: yes.
- [x] A3: The regression oracle is term_core fed the raw stream, with the
  removed construct's own effect excluded. Reason: this is the predecessor
  convention. Impact: low. Reversible: yes.
- [x] A4: Batch answer (batch-codex-consultation): residual 3 uses
  carry_query_state. CSI bytes stay unheld, an O(1) classification is
  carried, the completing byte is replaced with CSI_CLOSING, C0 bytes are
  written once and in order, `ESC[6` + DEL may remain, and split/whole byte
  identity is not required. The classification must follow term_core's
  transitions and response conditions. The `round4_cut_csi.rs:621`
  expectation changes. Reason: recorded per policy record_as_assumption.
  Impact: high. Reversible: yes.
- [x] A5: Batch answer: the escape-state concatenation is in scope (FR4),
  within one call and across calls. Neutralization happens at the removal
  position so the carried state never needs an Escape value. Remap is
  verified at the start of, inside and right after the removed span. Reason:
  recorded per policy record_as_assumption. Impact: medium. Reversible: yes.
- [x] A6: Batch answer: no closing at the snapshot end. The reattach trailing
  control sequence and the wrapped-ring dump block are unchanged. Pre-fix
  hot-upgrade rings are out of scope. Reason: recorded per policy
  record_as_assumption. Impact: medium. Reversible: yes.
- [x] A7: Batch answer: the design step is skipped. Reason: recorded per
  policy record_as_assumption. Impact: low. Reversible: yes.
- [x] A8: The predecessor's DECISIONS.md is not edited. Test-docs records are
  updated only for renamed tests, per the project rule. Reason: follows the
  predecessor practice and `.claude/rules/test-docs-records.md`. Impact: low.
  Reversible: yes.

### 14.2 Open Items

None. No requirement has `status: tbd`.

## 15. References

- `feature-docs/mux-cut-csi-post-strip-closure/DECISIONS.md`: the three FR6
  residuals this feature closes.
- `.claude/rules/test-docs-records.md`: test-docs record update rule.
