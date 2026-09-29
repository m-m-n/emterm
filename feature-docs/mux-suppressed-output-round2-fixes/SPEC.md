# Feature: mux-suppressed-output-round2-fixes

## Overview

Eight medium findings from review round 2 of mux-suppressed-output-fixes
(PR #109) remain `unresolved`. They all concern the side-effect handling of a
chunk that is suppressed because a snapshot capture (tab switch / reattach)
overlaps it. This feature makes that handling reach the same parse state, in
the same order, as the client would have reached had the chunk not been
suppressed, and records a decision for each finding.

Requirements document: `feature-docs/mux-suppressed-output-round2-fixes/REQUIREMENTS.md`.

## Objectives

- Make the side-effect handling of a suppressed chunk (assembly of the
  replacement output and the write filter's decisions) reach the same parse
  state, in the same order, as the client's parse of the stream had the chunk
  not been suppressed.
- For each of the 8 medium findings left `unresolved` in review round 2 of
  mux-suppressed-output-fixes (PR #109), decide whether it is resolved or
  needs no change, and record the rationale and the matching regression test.

## User Stories

### US1: Snapshot capture overlapping a chunk
As a mux user whose pane keeps producing output, I want a snapshot capture
(tab switch / reattach) that overlaps a chunk to leave query responses,
viewer launches and the display exactly as they would be without the
suppression.

**Acceptance Criteria:**
- [ ] AC-1 through AC-6 and AC-8 (see Success Criteria)

### US2: Decision record for review round 2
As the maintainer, I want every round-2 medium finding to carry a decision,
rationale and regression test mapping.

**Acceptance Criteria:**
- [ ] AC-7 (see Success Criteria)

## Technical Requirements

All requirements are `resolved`; none is `tbd`.

### Functional Requirements

- **FR1:** Exclude from the retention-window restart position any ESC that
  can fall in a designator slot (TM-1). Review finding `ecc48041b65a5380`.
    - When the retention window (256 bytes) is full, restart-position
      selection (`derive_prefix_start`) never chooses an ESC at a position
      that may be consumed as a designator.
    - The ESC at offset `i` is undecidable when `i == 0`, or when
      `window[i-1]` is `(` / `)` and either `i == 1` or `window[i-2]` is ESC.
    - Chains of the form `ESC ( ESC ( …` are excluded one by one under the
      same rule.
    - When no decidable ESC exists, the existing fallback (as-05) applies.
    - The exclusion may only cause misses; it never fabricates a query or a
      viewer launch.

- **FR2:** Include main-screen color queries in the replacement output.
  Review finding `dd56f3984c74cde1`.
    - An OSC color query completed in a suppressed chunk is included in the
      replacement output, exactly like CSI queries, regardless of whether it
      lies in a ring-written (main-screen) range, and is delivered once after
      the snapshot.
    - The exclusion by overlap with ring-written ranges (the
      `overlaps_ranges` check in `assemble_items`) is removed.

- **FR3:** The write filter carries a pending designator state across reads
  (TM-1). Review finding `ae48e7cd98084c19`.
    - When the bytes passed to `feed` end with `ESC (` / `ESC )`,
      `ScrollbackWriteFilter` keeps the "awaiting designator" state
      separately from `pending`.
    - The first byte of the next `feed` is unconditionally consumed as the
      designator, even when it is ESC.
    - For input split at any position, the emitted bytes and `pending` equal
      those produced by feeding the same input in one call.
    - The `pending` retention rules (OSC, DCS, APC and a trailing lone ESC)
      and the 512 KiB cap are unchanged.

- **FR4:** The write filter reflects termination by the removed screen-switch
  sequences (TM-1). Review finding `b600645f1fa94686`.
    - The write filter (or whatever determines `pending`) treats an
      in-progress OSC, DCS or APC, and a trailing lone ESC, as closed at the
      position of a screen-switch sequence (47/1047/1049 `h`/`l`) removed
      during main-screen range extraction, the same as the client does.
    - For input that contains a switch sequence, the `pending`
      post-condition holds: it retains neither the closed sequence nor the
      bytes after it.
    - A terminated sequence is not re-sent as the replacement output's tail.

- **FR5:** Limit the pending-based scan exclusion to the corresponding range
  of the original chunk. Review finding `f8b600bcc0ed55da`.
    - The range excluded from the replacement-output scan because of
      `pending` is only the range of the original chunk that corresponds to
      `pending`.
    - The approach of excluding everything past a single cut position
      (`tail_exclusion_start` and the scan's `limit_in_chunk`) is dropped.
    - Queries and viewer launches in intervening alternate-screen ranges are
      scanned and included in the replacement output.
    - Together with FR4, the excluded range falls within the last
      main-screen range.

- **FR6:** Pass carried-over sequences completed in the write filter to the
  replacement output. Review finding `03ccd5c7702db8db`.
    - A viewer launch or color query that began in an earlier read, was
      carried over in `pending`, and completed in the suppressed chunk is
      included in the replacement output and delivered once after the
      snapshot, regardless of the retention window length.
    - (1) When the write filter hands over a completed sequence, it also
      hands over its position in the original chunk. Because the write
      filter processes concatenated main-screen ranges, the position is
      mapped back to original-chunk coordinates.
    - (2) Deduplication removes only re-detection of the same sequence via
      the retention window (the replacement-output scan finding the same
      sequence). Distinct launches with identical content are not removed.
    - (3) Ordering relative to queries and launch sequences in
      alternate-screen ranges follows the original stream order.
    - (4) Selection of which kinds to deliver uses the shared identification
      of FR7.

- **FR7:** Recover OSC numbers and identify viewer launches in a shared
  layer. Review finding `a93dffe30438a693`.
    - OSC number recovery (accumulate the digits up to the first `;` in base
      10; non-digit characters before the first `;` remain as data) and
      viewer-launch identification are performed in a shared layer that does
      not depend on `mux::ipc`.
    - Scrollback stripping (ring writes and snapshot assembly) and
      replacement-output extraction share this layer's recovery and
      identification.
    - The stripping side's decision moves from string matching to numeric
      identification.
    - The identification result is kept separate from each consumer's
      selection of what to strip or deliver:
        - Stripping side: strips the `REPLAYABLE_VIEWER_KINDS` kinds,
          agent-status and OSC 9999 emterm-md; keeps other kinds such as fold,
          and emterm-mux.
        - Delivery side: delivers viewer launches except image.
    - An OSC whose accumulated number exceeds u16 is not identified (as-06).

- **FR8:** Do not re-send, in the replacement output, an incomplete tail that
  the snapshot already carried. Review finding `3eccc254dd278b33`.
    - Satisfied without changing the snapshot bytes.
    - (1) Scope: cases where the snapshot actually generated leaves the
      client parser in an incomplete-tail state (mid UTF-8, awaiting a
      designator after `ESC (` / `ESC )`, or an incomplete CSI). This
      includes the case where the ring has wrapped but the screen dump is
      empty, or dump-block generation failed and no trailing block is
      appended (as-02).
    - (2) The snapshot's shape and trailing parse state are not inferred from
      the current pane state. They are kept in the boundary record, as
      snapshot information tied to the destination and the suppression
      boundary, and passed to replacement-output assembly. Every snapshot path
      that records a boundary (visibility restore, reattach, on-demand)
      records information about the snapshot it generated.
    - (3) When the replacement output has no query or launch sequence, the
      tail the client is already in is not re-sent.
    - (4) When the replacement output has queries or launch sequences
      (coexistence), account for the leading query/launch ESC being consumed
      as a designator, CSI being aborted, and a mid-UTF-8 sequence being
      discarded, so that both query delivery and the parse state for
      subsequent chunks match the raw stream.
    - (5) Interference between daemon-appended bytes and the tail is out of
      scope (as-03).

- **FR9:** Record the decisions.
    - Place a per-`stable_id` decision table under
      `feature-docs/mux-suppressed-output-round2-fixes/`.
    - Each row states the verdict (resolved / no change needed), the
      rationale, and the matching regression test.
    - All 8 findings appear, one row each: `ecc48041b65a5380`→FR1,
      `dd56f3984c74cde1`→FR2, `ae48e7cd98084c19`→FR3,
      `b600645f1fa94686`→FR4, `f8b600bcc0ed55da`→FR5,
      `03ccd5c7702db8db`→FR6, `a93dffe30438a693`→FR7,
      `3eccc254dd278b33`→FR8.
    - `feature-docs/mux-suppressed-output-fixes/reviews/round2.yaml` is not
      modified.

### Non-Functional Requirements

- **NFR1 - Compatibility (wire format and snapshot bytes):** The `mux_ipc`
  wire format and the shape of Snapshot / SnapshotRestore frames are
  unchanged. Snapshot byte assembly for visibility restore, reattach and
  on-demand (each layout in `snapshot_bytes.rs`) is unchanged. The
  replacement output is sent as ordinary `PtyOutput` chunks. The only
  exception is the change in strip targets caused by FR7, stated explicitly
  here: numbers with leading zeros (e.g. `0777;emterm;markdown;…`,
  `09999;emterm-md`), and sequences that become identifiable because shared
  recovery keeps non-digit characters before the first `;` as data (e.g.
  `777emterm;;markdown;…`). These become stripped from the ring and the
  snapshot.
- **NFR2 - Lock discipline:** Lock order is `output_target` → capture
  exclusion → ring / shadow parser. No `blocking_send` while holding
  `output_target`. The snapshot information added to the boundary record
  (FR8) follows the same discipline; snapshot assembly and the trailing
  parse-state decision run outside the capture exclusion.
- **NFR3 - Reader hot-path load:** No new scan is added to the reader's
  normal path. FR3's awaiting-designator state and FR6's completion positions
  of carried-over sequences are obtained within the scan the write filter
  already performs. Identification, query/viewer-launch extraction and tail
  assembly run only for suppressed chunks.
- **NFR4 - Security (TM-1, TM-3):** Never fabricate a query or viewer launch
  from a position where the client parser does not start a control sequence.
  The replacement output is sent only to the destination that received that
  snapshot. FR8's snapshot information is taken from the record tied to that
  destination and suppression boundary, never decided by re-reading the
  current `output_target`.
- **NFR5 - Security (TM-2):** The replacement-output scan, retention-window
  restart-position selection, the write filter's boundary scan and the
  snapshot trailing parse-state decision are bounded single passes and do not
  panic. An empty replacement output is never turned into an empty
  `PtyOutput` chunk. The 512 KiB `pending` cap and the strip-filtered flush
  on exceeding it are unchanged.
- **NFR6 - Build and platforms:**
  `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml --no-default-features`
  passes. FR7's shared layer depends on neither the `gui` feature nor
  `mux::ipc`. Behavior is the same on Linux and Windows.

## Implementation Approach

### Architecture

Components named by the requirements:

- Reader normal path and `ScrollbackWriteFilter` (`write_filter.rs`) — FR3,
  FR4, FR6, NFR3.
- Replacement-output assembly (`suppressed_output.rs`: `assemble_items`,
  retention window / `derive_prefix_start`) — FR1, FR2, FR5, FR6, FR8.
- Client-parity scan (`client_parity_scan.rs`) — the scan whose rule basis is
  as-01.
- Snapshot assembly (`snapshot_bytes.rs` layouts, unchanged per NFR1) and the
  per-destination / per-suppression-boundary boundary record carrying the
  generated snapshot's information — FR8.
- A shared OSC number recovery and viewer-launch identification layer,
  independent of `mux::ipc` and the `gui` feature, used only by scrollback
  stripping and replacement-output extraction — FR7, as-09.

Lock order: `output_target` → capture exclusion → ring / shadow parser
(NFR2, as-04).

### Data Flow

Delivery order to the client (as-04):

```
snapshot → queries and viewer launches in original order → tail → subsequent chunk
```

### Dependencies

**Internal Dependencies:**
- term_core parser transitions and GUI theme response generation
  (`src-tauri/src/render/theme.rs`): the basis for "the same parse rules as
  the client"; the transition rule table (a)–(h) of the previous feature's
  SPEC FR2 is carried over (as-01).
- `extract_main_buffer_bytes` and the shadow parser: their handling of a
  switch sequence immediately after `ESC (` / `ESC )` is unchanged (as-08).
- `AgentStatusFeedScanner` and `PassthroughScanner`: their OSC detection is
  unchanged (as-09).

### File Structure

Locations named in the task:

```
src-tauri/src/mux/ipc/pty_spawn/
├── client_parity_scan.rs
├── suppressed_output.rs
├── write_filter.rs
└── mod.rs
```

## Declared Change Set

This section states the create-plan derivation instead of a hand-authored
list: the feature-specific paths above are derived at create-plan from
every task's `files` entries in `workflow.yaml`
(`references/phases/create-plan-phase.md`).

Every SPEC declares, by default, the following two workflow-generated
entries in addition to the feature-specific paths above:

- `feature-docs/mux-suppressed-output-round2-fixes/**`
- `test-docs/mux-suppressed-output-round2-fixes/**`

`feature-docs/mux-suppressed-output-round2-fixes/**` covers `REQUIREMENTS.md`,
`SPEC.md`, `IMPLEMENTATION.md`, `workflow.yaml`, `phase-state/`, `tasks/`,
`reviews/roundN.yaml`, `VERIFICATION.md`, `retrospect.yaml`, and the design
artifacts the design step produces. These are generated and owned by the
phase documents and by `references/phase-state.md`; this section cites them
and restates none of their rules.

`test-docs/mux-suppressed-output-round2-fixes/**` covers
`test-docs/mux-suppressed-output-round2-fixes/{T}.tests.yaml`, the per-task
test record. It is generated and owned by `implement-phase.md`; this section
cites it and restates none of its rules.

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
- [ ] TS-1 (FR1): Suppress each of: a full retention window `ESC ( ESC ]10;`
  plus 249 spaces followed by chunk `?` BEL; `ESC ( ESC [` plus 252 NULs
  followed by chunk `c`; an `ESC ( ESC ( …` chain. The replacement output and
  the responses are empty. When the window contains a decidable ESC (one not
  in a designator slot), scanning starts there as before and the query is
  delivered once.
- [ ] TS-3 (FR3): Feed input with no terminator (e.g. `ESC ( ESC ]11;?tail`)
  split right after `ESC (` in two `feed` calls. Emitted bytes and `pending`
  equal the single-call result. In the flow split input → suppression →
  re-send → BEL, no color-query response occurs.
- [ ] TS-4 (FR4): On the main screen, suppress a chunk that outputs
  `ESC ]11;?`, `ESC [?47h`, `ESC [?47l` and one space, then send BEL after the
  replacement output. No response occurs. No terminated OSC remains in
  `pending` after `feed`. Same for the 1047 and 1049 forms.
- [ ] TS-7 (FR7): Put `0777;emterm;markdown;…` and `09999;emterm-md;…` in a
  suppressed main-screen chunk. They are absent from the snapshot and
  delivered once in total by the replacement output; once as well in an
  alternate-screen range. `777emterm;;markdown;…` is stripped from the ring
  and identified as a viewer launch. `0777;emterm;image;…` is stripped from
  the ring and not delivered. agent-status is stripped; fold and emterm-mux
  are kept. An OSC whose number exceeds u16 is not identified.

### Integration Tests
- [ ] TS-2 (FR2): On the main screen, suppress a chunk containing
  `ESC ]11;?` BEL. Apply the snapshot with `reset_and_replay_segments` and a
  discarded `take_response`, then feed the replacement output. The response
  count is 1. Via the reader (visible reattach, on-demand, visibility restore)
  the main-screen color query is also delivered once.
- [ ] TS-5 (FR5): Suppress a chunk received on the main screen as
  `ESC ]2;x`, `ESC [?1049h`, `ESC [6n`, `ESC [?1049l`, `y`. The CSI 6n in the
  alternate-screen range is delivered once. When a genuinely incomplete
  sequence remains in the last main-screen range, only that sequence becomes
  the tail, and queries and viewer launches in the preceding alternate-screen
  range are also delivered.
- [ ] TS-6 (FR6): Split an OSC 777 emterm markdown longer than 256 bytes
  across reads and suppress the completing chunk. It is delivered once after
  the snapshot. A short launch whose start lies in the retention window,
  split the same way, is also delivered once (deduplication). Two launches
  with identical content are delivered twice. They are ordered with
  alternate-screen queries and launch sequences in the original order. A
  carried-over color query completing in the suppressed chunk is answered
  once.
- [ ] TS-8 (FR8): For visibility restore of a main-screen pane (ring not
  wrapped, and ring wrapped with an empty screen dump), split mid UTF-8,
  right after `ESC (`, and inside an incomplete CSI, and suppress the first
  half. The client's display, cursor and parsing of subsequent chunks match
  the reference, and no replacement character or designator `(` is shown.
  Coexistence: suppress a chunk that ends in the same tail after a query. The
  query is answered once and parsing of subsequent chunks matches the
  reference. The existing reattach-path tests
  (`visible_reattach_redelivers_a_cut_csi_tail_and_the_client_view_matches_the_reference`,
  `visible_reattach_redelivers_a_cut_utf8_tail_with_no_replacement_character_in_any_row`)
  pass unchanged.

### E2E Tests
**Existing E2E tests**: None
**Run command**: Not detected
- [ ] TS-10 (NFR6):
  `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml --no-default-features`
  passes.

### Edge Cases
- [ ] A sequence flushed because `pending` exceeded the 512 KiB cap is not
  treated as completed (not handed over under FR6).
- [ ] Under FR8 coexistence with an `ESC (` / `ESC )` tail, keep the leading
  query/launch ESC from being consumed as a designator, then restore the
  state in which the first byte of the subsequent chunk is consumed as the
  designator.
- [ ] A launch carried over under FR6 completes in the main-screen range of a
  suppressed chunk that ends by switching to the alternate screen.
- [ ] In consecutive suppressed chunks, the continuation of `pending` that
  the earlier replacement output re-sent completes in the next suppressed
  chunk (delivered only once after each snapshot).
- [ ] When snapshots are sent to the same destination back to back, the
  snapshot information of the boundary record covering the chunk is used.
- [ ] Inline images (Kitty APC, SIXEL DCS) and OSC 777 emterm image in a
  suppressed chunk are not delivered and remain a known loss (previous
  feature FR7).

### Performance Tests
- [ ] TS-9 (NFR5): With adversarial input (designator chains, repeated
  switch sequences and incomplete introducers, long carried-over sequences),
  replacement-output assembly and the write filter finish within the time
  budget and do not panic.

## Security Considerations

- **TM-1 (NFR4):** Never fabricate a query or viewer launch from a position
  where the client parser does not start a control sequence. FR1, FR3 and
  FR4 fix violations of this constraint.
- **TM-2 (NFR5):** Scans and decisions are bounded single passes and do not
  panic. No empty replacement output is sent. The `pending` cap is unchanged.
- **TM-3 (NFR4):** The replacement output and FR8's snapshot information are
  decided from the record tied to the destination that received the
  snapshot.
- **Authentication / Authorization / XSS / SQL Injection / CSRF:** Not
  applicable.

## Error Handling

No error codes are introduced. The scans and decisions listed in NFR5 do not
panic.

## Performance Optimization

### Performance Goals
- No new scan on the reader's normal path (NFR3).
- Scans and decisions are bounded single passes (NFR5).

### Optimization Strategies
- FR3's awaiting-designator state and FR6's completion positions are
  obtained within the write filter's existing scan (NFR3).
- Identification, query/viewer-launch extraction and tail assembly run only
  for suppressed chunks (NFR3).

## Assumptions

- **as-01:** The basis for "the same parse rules as the client" is the
  term_core parser transitions and the GUI theme
  (`src-tauri/src/render/theme.rs`) response generation. The transition rule
  table (a)–(h) of the previous feature's SPEC FR2 is carried over.
- **as-02:** FR8's scope is decided by whether the generated snapshot bytes
  leave the client parser in an incomplete-tail state. In the current
  assembly this is the main-screen visibility-restore snapshot without a dump
  block (ring not wrapped, or ring wrapped with an empty screen dump /
  generation failure). Reattach and on-demand snapshots (ending in
  `ESC[?1049l`), alternate-screen snapshots, and snapshots with a dump block
  end in daemon-appended bytes.
- **as-03:** Interference between daemon-appended bytes and the tail is not
  fixed in this feature: the trailing switch sequence of reattach / on-demand,
  the alternate-screen `ESC[?1049h` of visibility restore, dump blocks, etc.
  For those shapes the replacement output re-sends the tail as before.
- **as-04:** The previous feature's invariants hold: lock order; no
  `blocking_send` while holding `output_target`; the `mux_ipc` wire format;
  the 512 KiB `pending` cap; the retention window cap N = 256 bytes; the
  replacement output delivery order (snapshot → queries and viewer launches
  in original order → tail → subsequent chunk); removing C0 from an
  incomplete CSI tail.
- **as-05:** When the retention window has no decidable ESC, scanning starts
  from the ground state; continuing mid-string past N in a non-ring-written
  range is a known loss (previous feature as-05). More positions excluded by
  FR1 still err toward misses. Long sequences in ring-written ranges are
  handled by FR6.
- **as-06:** The u16 overflow in term_core's OSC number accumulation
  (`osc.rs:22`) is not fixed in this feature. The shared identification does
  not identify overflowed numbers.
- **as-07:** term_core, on receiving ESC mid UTF-8, discards the partial
  bytes without emitting a replacement character. Basis: the existing test
  `visible_reattach_redelivers_a_cut_utf8_tail_with_no_replacement_character_in_any_row`
  (the snapshot's trailing `ESC[?1049l` takes this shape). The term_core
  implementation is checked at create-plan.
- **as-08:** Main-screen range extraction (`extract_main_buffer_bytes`) and
  the shadow parser's handling are unchanged for a switch sequence
  immediately after `ESC (` / `ESC )` (the client consumes the switch
  sequence's ESC as the designator).
- **as-09:** OSC detection in `AgentStatusFeedScanner` and
  `PassthroughScanner` is unchanged. Only scrollback stripping and
  replacement-output extraction use FR7's shared layer.
- **as-10:** The two low `dropped_at_gate` findings in round2.yaml are out of
  scope. Of those, the color-query double loop disappears when FR2 removes
  `overlaps_ranges`.
- **as-11:** The additional launch instructions (pushing the integration
  branch and creating the PR, consulting Codex on open points and recording
  that in Notion) are treated as workflow operations, not functional
  requirements.

## Success Criteria

- [ ] AC-1: Each of the 8 findings has a regression test that fails on the
  pre-fix code and passes after the fix.
- [ ] AC-2: Feeding the snapshot (the real assembly function's output applied
  with `reset_and_replay_segments`), the replacement output and subsequent
  chunks into term_core yields the same result as a reference that feeds the
  raw stream once. Compared: screen, cursor, response bytes, displayed
  characters. Covers each case of FR3, FR4, FR5, FR6 and FR8, with varying
  split positions. No continuation bytes or replacement characters are
  displayed.
- [ ] AC-3: Queries in a suppressed chunk (CSI and color queries, including
  main-screen color queries) arrive exactly once each, in original order,
  after the snapshot and before the next chunk. The following do not arrive:
  queries that already reached the client; query-shaped bytes at positions
  where the client parser does not start a sequence (those starting at an ESC
  in a retention-window designator slot, and those after an
  awaiting-designator state spanning reads); terminated OSCs (including
  termination by a removed switch sequence).
- [ ] AC-4: A viewer launch completed in a suppressed chunk arrives exactly
  once across the snapshot and the replacement output, including launches
  that began before the retention window and launches with leading-zero
  numbers. Distinct launches with identical content each arrive once. Inline
  images and OSC 777 emterm image do not arrive.
- [ ] AC-5: The write filter's emitted bytes and `pending` equal the
  single-call result regardless of split position, including unterminated
  input split right before the designator. The `pending` post-condition
  holds for input containing a switch sequence.
- [ ] AC-6: Snapshot bytes for visibility restore, reattach and on-demand are
  identical to before, except for NFR1's exception (FR7's strip-target
  change). The `mux_ipc` wire format is unchanged.
- [ ] AC-7: For each of the 8 `stable_id`s, verdict, rationale and
  regression-test mapping are recorded in the decision table under
  `feature-docs/mux-suppressed-output-round2-fixes/`.
  `feature-docs/mux-suppressed-output-fixes/reviews/round2.yaml` is
  unchanged.
- [ ] AC-8: Existing tests other than those intentionally changing behavior
  pass unchanged. Behavior-changing tests are listed (expected:
  `osc_color_query_inside_ring_written_ranges_is_not_redelivered` in
  `suppressed_output.rs`;
  `visible_reattach_redelivers_an_alt_screen_color_query_but_not_one_that_reached_the_ring`
  and
  `on_demand_snapshot_redelivers_an_alt_screen_color_query_but_not_one_that_reached_the_ring`
  in `pty_spawn/tests.rs`). Rewriting call sites due to argument / return
  value changes does not count as a behavior change.
- [ ] AC-9:
  `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib`
  and
  `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml --no-default-features`
  pass.

## Open Questions

None. No requirement has `status: tbd`.

## References

- Requirements document: `feature-docs/mux-suppressed-output-round2-fixes/REQUIREMENTS.md`
- Review round 2 findings: `feature-docs/mux-suppressed-output-fixes/reviews/round2.yaml`
  on branch `em-workflow/mux-suppressed-output-fixes/integration`
