# Feature: mux-suppressed-output-round3-fixes

## Overview

This feature resolves the 8 medium findings left `unresolved` in review round 1
of mux-suppressed-output-round2-fixes (PR #111). It changes the mux daemon's
write filter, the shared scrollback strip, the per-destination boundary record
and the suppressed-chunk replacement output, and records a verdict for each
finding. Requirements document: `feature-docs/mux-suppressed-output-round3-fixes/REQUIREMENTS.md`.

## Objectives

- Make the side-effect handling of a chunk suppressed by an overlapping snapshot capture (tab switch / reattach) reach the same parse state, in the same order, as the client would have reached had the chunk not been suppressed.
- Remove the extra copy and UTF-8 validation that the normal (non-suppressed) scrollback write path now pays for every OSC.
- For each of the 8 medium findings left `unresolved` in review round 1 of mux-suppressed-output-round2-fixes (PR #111), decide whether it is resolved or needs no change, and record the rationale and the matching regression test.

## Acceptance Criteria

- [ ] **AC-1:** Each of the 8 findings has a regression test that fails on the pre-fix code and passes after the fix.
- [ ] **AC-2:** This check feeds term_core three things: the snapshot (the real assembly function's output, applied with `reset_and_replay_segments`), then the replacement output, then the subsequent chunks. The result matches a reference that feeds the raw stream once, comparing screen, cursor, response bytes and displayed characters. Covers the cases of FR1 (no trailing space after the closed OSC, for 47/1047/1049), FR4, FR5, FR6 and FR7. No replacement character and no stray designator `(` is displayed.
- [ ] **AC-3:** No color query or CSI query is delivered or answered in any of these cases: an OSC closed by a removed switch followed by a BEL in a later read (FR1); a switch sequence straddling read boundaries with the completing read suppressed (FR4); a 129-long `ESC (` chain followed by a suppressed `ESC ]11;? BEL` (FR5).
- [ ] **AC-4:** For all split positions and byte-at-a-time feeding, the write filter's emitted bytes and `pending` equal the single-call result. The corpus includes `ESC ( ESC [6n` and `ESC ( ESC ]777;emterm;markdown;x BEL` (FR2).
- [ ] **AC-5:** `ESC ( ESC ]0777;emterm;markdown;begin;id=x BEL X` keeps its displayed bytes in both the ring write strip and the snapshot strip, fed in one call or split (FR3).
- [ ] **AC-6:** When a second snapshot is recorded for the same destination between the covered decision and the send, the tail-omission decision uses the record current at send time. No UTF-8 continuation byte becomes a replacement character, and an omitted tail is sent when the current record requires it, including when the stale decision gave an empty replacement (FR6). At an equal boundary the later record's construct is used, in both orders of on-demand and visibility restore (FR7).
- [ ] **AC-7:** The new identification performs zero heap allocations, measured by a test-only thread-local counter around its calls only. Its result equals `identify_osc(&recover_osc(body))` over a corpus that includes canonical, leading-zero, non-digit-before-`;`, overflow, invalid-UTF-8, empty and long bodies (FR8).
- [ ] **AC-8:** Existing tests pass unchanged, except the tests whose expectation changes on purpose; those are listed in the decision table. Expected: `ac2_equal_boundary_with_a_different_construct_yields_none` (session/pane/tests.rs, FR7), plus `a_cut_closes_an_in_progress_osc_and_pending_holds_no_closed_osc`, `a_cut_closes_a_held_lone_esc`, `an_empty_fed_range_with_a_cut_still_closes_pending` and `pending_after_a_cut_equals_a_fresh_scan_of_the_bytes_after_the_last_cut` (pty_spawn/tests.rs, FR1). Rewriting call sites because of argument or return-value changes does not count as a behavior change. A renamed test listed in a predecessor's `test-docs/*/taskNNNN.tests.yaml` is updated there per `.claude/rules/test-docs-records.md`, with a supersede note when the expectation is inverted.
- [ ] **AC-9:** `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib` and `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml --no-default-features` pass.
- [ ] **AC-10:** The decision table under `feature-docs/mux-suppressed-output-round3-fixes/` records verdict, rationale and regression test for all 8 stable_ids. `feature-docs/mux-suppressed-output-round2-fixes/reviews/round1.yaml` is unchanged.

## Technical Requirements

### Functional Requirements

- **FR1:** A construct closed by a removed screen switch is not written to the ring (review finding 973af79f520bdaa5). When the write filter closes an incomplete OSC, DCS or APC string, or a held lone ESC, at a cut (the position of a screen-switch sequence 47/1047/1049 h/l removed during main-screen range extraction), none of that construct's bytes, from its opening ESC on, are written to the scrollback ring. The settled bytes before that ESC still go through the existing strip and are written. No terminator (ST or any other) is appended. This applies whether the construct was carried over in `pending` from an earlier read or opened in the current segment. Consequence: replaying a main-screen visibility-restore snapshot never leaves the client inside an OSC, so a BEL in a later read cannot complete a color query. Basis: answer req.fr1-closed-construct-handling (option (a)).
- **FR2:** The shared strip consumes charset designators, and the write filter passes its awaiting-designator state (review finding 195916fd94088018). The shared strip (`strip_rich_content_and_remap` and its write-path and snapshot-path entry points) consumes the byte after `ESC (` / `ESC )` as the designator unconditionally, even when that byte is ESC, the same transition as term_core's escape handling. The strip accepts an initial awaiting-designator state. The write filter passes the state in effect at the start of each strip call on every path (normal drain, cut, and overflow flush), and the byte-to-position correspondence (remapped offsets) is preserved across the designator byte. The snapshot side calls the strip with no initial state (ground). Consequence: for any split position, and for byte-at-a-time feeding, the write filter's emitted bytes and `pending` equal those from feeding the same input in one call, including `ESC ( ESC [6n` and `ESC ( ESC ]777;emterm;markdown;x BEL`. Basis: answer req.fr2-fr3-shared-strip (option (a)).
- **FR3:** The strip side never takes a designator ESC as an introducer (review finding 76342d8d941e6416). Met by the same shared-strip change as FR2: an ESC consumed as the designator after `ESC (` / `ESC )` is not identified as the start of an OSC, APC, DCS or CSI candidate on the strip side. So `ESC ( ESC ]0777;emterm;markdown;begin;id=x BEL X` keeps its displayed bytes in both the ring write and the snapshot strip, whether it is fed in one call or split. Basis: answer req.fr2-fr3-shared-strip (option (a)).
- **FR4:** The read-boundary-straddling fallback path closes what the write filter holds (review finding 2a9d929f4fafb7cb). On the reader's fallback path (the main-span extraction's final alt state disagrees with the shadow parser's post-chunk state, `pty_spawn/mod.rs` around lines 519-529), when `alt_before` or `alt_after` is true, the write filter closes `pending` and clears the awaiting-designator state the same way a cut does (for example, an empty fed range with one cut), with FR1's handling of the closed construct. Consequence: when a switch sequence straddles read boundaries (for example, reads `ESC` / `[?1049h` / `ESC` / `[?1049l` / `]11;? BEL`) and the last read is suppressed, no carried-over completion is reported and no color query is delivered. The fallback for `!alt_before && !alt_after` (whole-chunk feed) is unchanged.
- **FR5:** The as-05 fallback does not fabricate across an undecidable trailing `ESC (` (review finding eaf83fe08869d5e6). In `derive_prefix_start`'s as-05 fallback (the retention window is full and holds no decidable ESC), the scan treats the chunk's first byte as possibly consumed as a designator when the window ends with `(` / `)` and that byte is either the window's first byte or preceded by ESC. In that case no item and no tail is extracted from a construct that starts at the chunk's first byte. This can only cause misses, never fabrication. Consequence: after 129 consecutive `ESC (` in earlier reads, a suppressed chunk `ESC ]11;? BEL` delivers no query and produces no response.
- **FR6:** Generation-checked tail-omission decision at send time (review finding 6230e66b979e312e). Each per-destination boundary record carries a generation (or an equivalent identity) that advances on every record, including a re-record at the same boundary. The reader's covered decision captures the generation together with the construct. When sending the replacement output, the reader first secures a send slot outside every exclusion. It then re-takes `output_target`, then the boundary exclusion, and re-checks the destination's record. The tail-omission decision uses the record current at that moment (its construct applies only when the chunk number equals the recorded boundary). The reader holds that hold through `permit.send`. Scanning and notification processing stay outside the exclusions. The re-check also runs when the replacement would be empty under the stale construct, so a tail the earlier decision omitted is still sent if the current record requires it. An empty final replacement is never sent. This applies to both suppressed-pipeline call sites (the direct Suppressed arm and the NeedsSlot re-check arm). Basis: answer req.fr6-recheck-at-send (option (a)).
- **FR7:** At an equal boundary the later record wins (review finding 240031761bfdf695). When a boundary equal to the destination's existing boundary is recorded, the later record replaces the construct, including a replacement by none. Conflicting constructs no longer collapse to none. A higher boundary still replaces both values, and a lower boundary still leaves the entry unchanged. Each such record advances the FR6 generation. Consequence: an on-demand snapshot (construct none) followed by a visibility-restore snapshot (`ESC (`) at the same boundary leaves `ESC (`, and the replacement does not re-send `ESC (`. The reverse order leaves none, and the tail is re-sent.
- **FR8:** Allocation-free OSC identification on the normal write path (review finding b3e644c5e2d31809). `crate::mux::osc_identify` gains an identification entry point that takes the OSC body bytes and performs no heap allocation. It recovers the number by the unchanged rule: digits before the first `;` accumulate in base 10; non-digit bytes before the first `;` are data; the first `;` is dropped; an accumulation above u16 is not identified. It returns NotIdentified immediately when the number is neither 777 nor 9999. Otherwise it compares only the prefix needed to decide the kind, using borrowed bytes. Its result equals `identify_osc(&recover_osc(body))` for every body. The strip's `is_replayable_osc_body` uses it. `recover_osc` / `identify_osc` are kept, both as the reference for the equivalence test and for delivery-side callers. Basis: answer req.fr8-regression-test (option (a)).
- **FR9:** Record the decisions. Place a per-stable_id decision table under `feature-docs/mux-suppressed-output-round3-fixes/`. Each row states the verdict (resolved / no change needed), the rationale and the matching regression test. All 8 findings appear, one row each: 973af79f520bdaa5->FR1, 195916fd94088018->FR2, 76342d8d941e6416->FR3, 2a9d929f4fafb7cb->FR4, eaf83fe08869d5e6->FR5, 6230e66b979e312e->FR6, 240031761bfdf695->FR7, b3e644c5e2d31809->FR8. Tests whose expectation changes on purpose are listed with old name, new name, file and reason. `feature-docs/mux-suppressed-output-round2-fixes/reviews/round1.yaml` is not modified.

### Non-Functional Requirements

- **NFR1 - Compatibility:** The `mux_ipc` wire format and the shape of Snapshot / SnapshotRestore frames are unchanged. Snapshot byte assembly layouts for visibility restore, reattach and on-demand are unchanged, and the replacement output is still sent as ordinary `PtyOutput` chunks. Stated exceptions in ring and snapshot content: (a) a construct closed by a cut, or by the FR4 fallback closing, is no longer written to the ring (FR1/FR4); (b) an ESC consumed as a designator after `ESC (` / `ESC )` no longer starts a strip target, so bytes that were stripped in that position are now kept (FR2/FR3).
- **NFR2 - Lock discipline:** Lock order is `output_target` -> capture exclusion -> ring / shadow parser, and `output_target` -> boundary exclusion. No blocking wait (`reserve_owned`, `blocking_send`) runs while `output_target` or the boundary exclusion is held. A non-blocking `permit.send` under the hold is allowed, as on the existing NeedsSlot path. Replacement scanning, the trailing-construct decider and notification processing run outside the capture and boundary exclusions.
- **NFR3 - Reader hot-path load:** No new scan is added to the reader's normal path. Designator tracking happens inside the strip's existing single pass. FR8 removes the per-OSC copy and UTF-8 validation from the write path. The generation adds O(1) work per record and per covered check. Identification for delivery, item extraction and tail assembly run only for suppressed chunks.
- **NFR4 - Security (TM-1, TM-3):** Never fabricate a query or viewer launch from a position where the client parser does not start a control sequence. FR1, FR2, FR4 and FR5 fix violations of this constraint. The replacement output goes only to the destination that received the covering snapshot. The construct used for tail omission comes from that destination's current boundary record and is never decided by re-reading another destination's state.
- **NFR5 - Security (TM-2):** The strip, the write filter's boundary scan, retention-window restart selection, the replacement scan and the new identification are bounded single passes and never panic. An empty replacement is never turned into an empty `PtyOutput` chunk. The 512 KiB `pending` cap and the strip-filtered flush on exceeding it are unchanged.
- **NFR6 - Build and platforms:** `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml --no-default-features` passes. `osc_identify` depends on neither `mux::ipc` nor the `gui` feature. The allocation counter used by the FR8 test exists only in test builds (`cfg(test)`). Behavior is the same on Linux and Windows.

## Assumptions

| ID | Assumption | Reason | Impact | Reversible | Related questions |
|----|------------|--------|--------|------------|-------------------|
| as-01 | Round2 invariants hold: lock order, no blocking wait under `output_target`, the `mux_ipc` wire format, the 512 KiB pending cap, the 256-byte retention window, replacement delivery order (snapshot -> queries and launches in original order -> tail -> subsequent chunk), and the term_core transition rules as the client-parity basis. | Carried over from feature-docs/mux-suppressed-output-round2-fixes/SPEC.md as-01/as-04. | medium | yes | - |
| as-02 | FR1's "incomplete construct closed by a cut" covers OSC/DCS/APC strings and a held lone ESC, which is the set the write filter's pending postcondition defines. An awaiting-designator `ESC (` at a cut is not such a construct and keeps its current handling (round2 as-08). | The answer req.fr1-closed-construct-handling does not enumerate the incomplete constructs. The pending contract in write_filter.rs defines incomplete constructs as exactly these kinds. A lone ESC written to the ring would combine with the next main-span byte into an escape the client never saw. | medium | yes | req.fr1-closed-construct-handling |
| as-03 | FR5 uses the window-only rule (treat the chunk's first byte as possibly a designator). It does not consult the write filter's awaiting-designator state on the main screen. | The window-only rule errs only toward misses (round2 as-05) and needs no new state handed to the suppressed path. The review lists the filter state as optional. | low | yes | - |
| as-04 | FR8's scope is the strip side (`is_replayable_osc_body`). Delivery-side callers (`client_parity_scan`, `suppressed_output`) may keep `recover_osc`, because they run only for suppressed chunks and color-query detection needs the recovered data. | Finding b3e644c5e2d31809 concerns the normal write path. The answer limits measurement to the new identification function. | low | yes | req.fr8-regression-test |
| as-05 | The FR4 closing on the fallback path uses the same semantics as a cut at fed position 0, including FR1's not-written rule. | The client saw the straddling switch's ESC, which closes any held string exactly as at a removed switch. | medium | yes | - |
| as-06 | The predecessor test-docs records (`test-docs/mux-suppressed-output-round2-fixes/*.tests.yaml`) may list the tests named in AC-8. Create-plan checks those records and applies the test-docs-records update duty to every renamed test. | Those records were not among this dispatch's inputs. | low | yes | - |

## Implementation Approach

### Architecture

Not applicable as a layered architecture. The design step was skipped: this is
a Rust-only change inside the mux daemon (write filter, scrollback strip,
boundary record, suppressed-chunk replacement). No UI, child WebView or visual
surface is touched.

**Components named by the requirements:**
```
Shared strip      strip_rich_content_and_remap (write-path and snapshot-path entry points),
                  is_replayable_osc_body                                 FR2, FR3, FR8
Write filter      pending, cut handling, normal drain, overflow flush    FR1, FR2, FR4
Reader            fallback path (pty_spawn/mod.rs around lines 519-529)  FR4
Suppressed path   derive_prefix_start as-05 fallback                     FR5
                  Suppressed arm and NeedsSlot re-check arm              FR6
Boundary record   per-destination boundary, construct, generation        FR6, FR7
OSC identify      crate::mux::osc_identify (new allocation-free entry,
                  recover_osc / identify_osc kept)                       FR8
```

### Lock Order (NFR2)

```
output_target -> capture exclusion -> ring / shadow parser
output_target -> boundary exclusion
```

### Data Flow (FR6 send sequence)

```
covered decision (construct + generation)
  -> secure send slot (outside every exclusion)
  -> re-take output_target -> re-take boundary exclusion
  -> re-check destination's current record -> tail-omission decision
  -> permit.send (hold kept through the send; empty final replacement is not sent)
```

### API Design

No external API change. The `mux_ipc` wire format and the Snapshot /
SnapshotRestore frame shape are unchanged (NFR1).

### Database Schema

Not applicable.

### Dependencies

**Internal Dependencies:**
- term_core: its escape-handling transitions are the client-parity basis (FR2, as-01).
- mux-suppressed-output-round2-fixes: its invariants are carried over (as-01).

**External Dependencies:**
- None.

## Declared Change Set

This section states the create-plan derivation instead of a hand-authored
list: the feature-specific paths above are derived at create-plan from
every task's `files` entries in `workflow.yaml`
(`references/phases/create-plan-phase.md`).

Every SPEC declares, by default, the following two workflow-generated
entries in addition to the feature-specific paths above:

- `feature-docs/mux-suppressed-output-round3-fixes/**`
- `test-docs/mux-suppressed-output-round3-fixes/**`

`feature-docs/mux-suppressed-output-round3-fixes/**` covers `REQUIREMENTS.md`, `SPEC.md`,
`IMPLEMENTATION.md`, `workflow.yaml`, `phase-state/`, `tasks/`,
`reviews/roundN.yaml`, `VERIFICATION.md`, `retrospect.yaml`, and the design
artifacts the design step produces. These are generated and owned by the
phase documents and by `references/phase-state.md`; this section cites them
and restates none of their rules.

`test-docs/mux-suppressed-output-round3-fixes/**` covers `test-docs/mux-suppressed-output-round3-fixes/{T}.tests.yaml`, the
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
- [ ] **TS-2** (FR2): Write-filter split invariance: for `ESC ( ESC [6n` and `ESC ( ESC ]777;emterm;markdown;x BEL` (added to the existing corpus), every split position and byte-at-a-time feeding give the same emitted bytes and `pending` as one call. Also cover the cut and overflow paths with a designator-start state.
- [ ] **TS-3** (FR3): Ring-write strip and snapshot strip of `ESC ( ESC ]0777;emterm;markdown;begin;id=x BEL X` keep the bytes, in one call and split. Remapped watch offsets stay consistent across the designator byte.
- [ ] **TS-5** (FR5): 129 consecutive `ESC (` in earlier reads (retention window full, no decidable ESC), then a suppressed chunk `ESC ]11;? BEL`. The replacement is empty and no response occurs. A window whose trailing `(` is not preceded by ESC still scans from the chunk start as before.
- [ ] **TS-7** (FR7): Same destination and boundary: on-demand (none) then visibility restore (`ESC (`) leaves `ESC (` and no `(` is re-sent. Visibility restore then on-demand leaves none and the tail is re-sent. Every re-record advances the generation.
- [ ] **TS-8** (FR8): Test-only thread-local allocation counter wrapped around the new identification calls only: zero allocations. Equivalence with `identify_osc(&recover_osc(body))` over the osc_identify corpus plus invalid UTF-8, overflow and long bodies.

### Integration Tests
- [ ] **TS-1** (FR1): On the main screen (ring not wrapped), suppress a chunk of `ESC ]11;?` followed by a 47/1047/1049 h/l pair with no trailing space, then a read with BEL. No response occurs and the ring holds no unterminated OSC. Repeat with a trailing space, with the OSC carried over in pending from an earlier read, and with a lone ESC before the switch. Compare with the raw-stream reference.
- [ ] **TS-4** (FR4): Reads `ESC` / `[?1049h` / `ESC` / `[?1049l` / `]11;? BEL` (and the other switch forms) with the last read suppressed. No carried-over completion is reported, the replacement holds no color query, and no response occurs.
- [ ] **TS-6** (FR6): Race test using the reader pause hooks: after the covered decision for a chunk, a second snapshot is recorded for the same destination (higher boundary, and a re-record at the same boundary) before the replacement is sent. The tail-omission decision follows the current record, no replacement character appears, and a tail omitted under the stale construct is sent when the current record requires it, including the empty-replacement case.

### E2E Tests
**Existing E2E tests**: None (no E2E inputs were resolved for this feature)
**Run command**: Not detected

### Edge Cases
- [ ] **EC-1** (FR1, FR6): A completed carried-over sequence before a cut is still reported (FR6 of round2) while the incomplete construct after it, closed by the cut, is not written.
- [ ] **EC-2** (FR1): An awaiting-designator `ESC (` at a cut keeps its current handling (round2 as-08); only OSC/DCS/APC strings and a lone ESC are dropped.
- [ ] **EC-3** (FR2): The overflow flush starting in the awaiting-designator state passes that state to the strip and leaves the flag equal to the client-parity state at the end of the run.

### Performance Tests
- [ ] **TS-9** (NFR5): Adversarial input (long designator chains, repeated switch sequences straddling reads, long carried-over OSC near the 512 KiB cap) finishes within the time budget and does not panic.

### Build Checks
- [ ] **TS-10** (NFR6): `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml --no-default-features` passes.

## Security Considerations

- **Fabrication (TM-1, TM-3):** See NFR4. No query or viewer launch is fabricated from a position where the client parser does not start a control sequence; replacement output goes only to the destination that received the covering snapshot, and the tail-omission construct comes from that destination's current boundary record.
- **Resource bounds (TM-2):** See NFR5. All scans are bounded single passes and never panic; the 512 KiB `pending` cap is unchanged.
- **Authentication / Authorization / XSS / SQL Injection / CSRF:** Not applicable.

## Error Handling

No new error codes. Per NFR5, the affected scans never panic, and an empty
replacement is never sent as an empty `PtyOutput` chunk (FR6, NFR5).

## Performance Optimization

See NFR3: no new scan on the reader's normal path, designator tracking inside
the strip's existing single pass, removal of the per-OSC copy and UTF-8
validation from the write path (FR8), O(1) generation work per record and per
covered check, and suppressed-only identification, item extraction and tail
assembly.

## Success Criteria

- [ ] All functional requirements are implemented and tested
- [ ] All test scenarios pass
- [ ] AC-1 through AC-10 are met
- [ ] The decision table records all 8 stable_ids (FR9, AC-10)

## Open Questions

> **Note**: 未解決の要件は workflow.yaml で `status: tbd` として管理されています。
> plan フェーズの実行前に解決してください。

None. Every requirement is resolved.

## References

- Requirements document: `feature-docs/mux-suppressed-output-round3-fixes/REQUIREMENTS.md`
- Predecessor SPEC: `feature-docs/mux-suppressed-output-round2-fixes/SPEC.md`
- Predecessor review round 1: `feature-docs/mux-suppressed-output-round2-fixes/reviews/round1.yaml`
- Test-docs records rule: `.claude/rules/test-docs-records.md`
