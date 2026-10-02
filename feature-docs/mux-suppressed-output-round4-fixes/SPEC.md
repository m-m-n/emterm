# Feature: mux-suppressed-output-round4-fixes

## Overview

This feature resolves the 3 medium findings left `unresolved` in review round 1
of mux-suppressed-output-round3-fixes (PR #112): 3e2024dce619ed9f,
989ec5c588abce06 and 48caec6f5b0b5810. It changes the mux daemon's write
filter, the client-parity scan and, if the adjacent paths reproduce, main-span
extraction. It records a verdict for each finding and a separate outcome for
two adjacent paths. Requirements document: `feature-docs/mux-suppressed-output-round4-fixes/REQUIREMENTS.md`.

## Objectives

- A chunk suppressed by an overlapping snapshot capture (tab switch / reattach), and the bytes written to the scrollback ring for it, leave the client in the same parse state as the live client, so replay or extraction never completes a query (OSC 11 color query, CSI DSR, etc.) the client did not start.
- For each of the 3 medium findings left unresolved in review round 1 of mux-suppressed-output-round3-fixes (PR #112) — 3e2024dce619ed9f, 989ec5c588abce06, 48caec6f5b0b5810 — decide whether it is resolved or needs no change, and record the rationale and the matching regression test.
- Check two adjacent paths (an in-progress CSI at a cut; a switch sequence directly after `ESC (` / `ESC )` removed by extraction) with term_core-based regression tests, fix the ones that reproduce, and record the outcome separately from the three stable_id verdicts.

## Acceptance Criteria

- [ ] **AC-1:** Each of the 3 findings (3e2024dce619ed9f, 989ec5c588abce06, 48caec6f5b0b5810) has a regression test that fails on the pre-fix code and passes after the fix.
- [ ] **AC-2:** A term_core comparison feeds the snapshot (the real assembly function's output, applied with `reset_and_replay_segments`, responses discarded), then the replacement, then the following chunks. The result matches a reference that feeds the raw stream once, comparing screen, cursor, response bytes and displayed characters. It covers FR1's three forms (`ESC]11;? ESC]0;x`, `ESC]11;? ESC ESC`, `X ESC ESC` before a 47/1047/1049 switch, followed by BEL, `\` and `]11;? BEL` respectively), each in one read and with the chain head in an earlier read. For FR3, the parse of the following bytes and the responses match the reference on every path (same call, earlier read with empty pending, overflow flush, mod.rs fallback). Screen differences that come only from FR5's path are judged under FR5.
- [ ] **AC-3:** No color query or CSI query is delivered or answered for the three task examples: `ESC]11;? ESC]0;x` + switch (FR1), `ESC ( ESC ( ESC ]11;? BEL` and `ESC ( ESC ( ESC [` + `6n` in the as-05 fallback (FR2), and `X ESC ( ESC[?1049h ESC[?1049l ESC ( ESC ]11;? BEL` (FR3).
- [ ] **AC-4:** For every split position and for byte-at-a-time feeding, the write filter's total emitted bytes and its `pending` equal the single-call result. The corpus includes aborted-string chains, `ESC ESC` chains, and chains of DCS/APC strings. After every call, `pending` satisfies the restated FR1 postcondition.
- [ ] **AC-5:** In the as-05 fallback, an item or tail that both readings report identically is still delivered. The existing round3 FR5 cases (`round3_as05_*`) keep their results.
- [ ] **AC-6:** FR4 and FR5 each have a term_core-based regression test. The decision record states for each whether it reproduced. For each one that reproduced, its test fails on the pre-fix code and passes after the fix.
- [ ] **AC-7:** Existing tests pass unchanged, except tests whose expectation changes on purpose; the decision record lists those. Expected candidates: the EC-2 parts of `a_cut_drops_the_construct_from_its_opening_esc_on` (round3_write_path.rs), `a_cut_clears_the_awaiting_designator_flag` and `pending_after_a_cut_equals_a_fresh_scan_of_the_bytes_after_the_last_cut` (pty_spawn/tests.rs), plus any test that pins pending content for an aborted chain or the overflow-path designator state. Rewriting call sites for signature changes does not count. A renamed test listed in a predecessor's `test-docs/*/taskNNNN.tests.yaml` is updated there per `.claude/rules/test-docs-records.md`, with a supersede note when the expectation is inverted.
- [ ] **AC-8:** `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib` and `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml --no-default-features` pass.
- [ ] **AC-9:** The decision table under `feature-docs/mux-suppressed-output-round4-fixes/` records verdict, rationale and regression test for all 3 stable_ids. The FR4/FR5 outcomes are recorded in a separate section. `feature-docs/mux-suppressed-output-round3-fixes/reviews/round1.yaml` is unchanged.

## Technical Requirements

### Functional Requirements

- **FR1:** Chain-aware closing at a cut (review finding 3e2024dce619ed9f). The write filter's boundary scan records two positions for the end of the fed run: the start of the last incomplete construct (an OSC/DCS/APC string or a lone ESC), and the head of the chain that leads into it. The chain head is the earliest opening ESC of consecutive constructs that are each closed by the ESC that opens the next one: a string aborted by `ESC <byte other than \>` whose aborting ESC opens the next construct, and the superseded first ESC of `ESC ESC`. `pending` holds the bytes from the chain head on. This holds on the normal drain path (write_filter.rs around line 355) as well as at a cut, so a chain that spans reads stays held: the chain head is not written on the read that opened it. At a cut, and when the round3 FR4 fallback in pty_spawn/mod.rs closes `pending`, no byte from the chain head on is written to the ring. The settled bytes before the chain head go through the existing strip. No terminator is appended. The pending postcondition (the `feed_with_cuts` / `pending()` docs) becomes: after every call, `pending` is empty or holds exactly the chain that ends in the single incomplete construct at the end of the fed stream after its last cut. The carried-over completion (`CarriedCompletion`) is defined from the construct start, not from the chain head. When the held incomplete construct completes inside the call before any cut, it is reported, and the whole held run is settled. The suppressed-chunk tail re-delivery (the `pending_after` tail in suppressed_output.rs, and the excluded pieces derived from its length) is aligned with the new `pending` content, so that no byte is both extracted as an item and re-delivered. The state the filter carries across the 512 KiB overflow flush (`pending`, chain head, construct start, awaiting-designator flag) is defined and tested. The overflow flush still passes the run through the strip. Consequence: `ESC]11;? ESC]0;x` + switch + BEL, `ESC]11;? ESC ESC` + switch + `\`, and `X ESC ESC` + switch + `]11;? BEL` produce no color query on replay. This holds whether the bytes before the switch arrive in one read or split across reads. Basis: answer requirement.fr1.cross-call-chain (hold-chain).
- **FR2:** As-05 fallback: designator undecidability carried through the chunk (review finding 989ec5c588abce06). When `first_chunk_byte_may_be_designator` holds, the client-parity scan reads the chunk two ways: (a) the chunk's first byte is consumed as the designator, and (b) the first byte is parsed from ground. An item is reported only when both readings report it with the same range. A tail is reported only when both readings give the same tail start. The tail's C0-strip flag comes from that tail. Anything else is dropped. This replaces the current rule, which only drops what starts at the chunk's first byte. Nothing changes outside the as-05 fallback condition. The rule can only cause misses, never add an item. Consequence: in the fallback, `ESC ( ESC ( ESC ]11;? BEL` delivers no color query, and `ESC ( ESC ( ESC [` re-delivers no tail that a following `6n` could complete into a DSR.
- **FR3:** Awaiting designator at a cut writes the consumed designator ESC (review finding 48caec6f5b0b5810). Sometimes the write filter closes at a cut, or at the round3 FR4 fallback closing in pty_spawn/mod.rs, while the client is waiting for a charset designator. This happens when the run ends in `ESC (` / `ESC )` outside any string, or when the flag was carried in with an empty `pending`. In that case the filter writes one ESC byte (0x1B) to the ring after the bytes already written, and clears the flag. That ESC stands for the switch sequence's ESC, which the client consumed as the designator. The same rule applies on every path: a cut in the same call, a wait left by an earlier read with an empty `pending`, the 512 KiB overflow flush followed by a cut, and the mod.rs fallback. Nothing is written when the client is not waiting for a designator. This supersedes round3 SPEC as-02 and EC-2, where the waiting `ESC (` was written with nothing after it. The existing tests that pin the old behavior change their expectations (see AC-7). Consequence: `X ESC ( ESC[?1049h ESC[?1049l ESC ( ESC ]11;? BEL` produces no OSC 11 query on replay. Verification compares term_core's parse of the following bytes and its responses with the raw-stream reference, not only the tail check. Basis: answer requirement.fr3.awaiting-designator-at-cut (append-esc).
- **FR4:** Conditional: an in-progress CSI at a cut (adjacent path). Create-plan first adds a term_core-based regression test for a CSI still in progress right before a cut. Example: `ESC[6`, then a 47/1047/1049 h/l switch, then `n` in a later read, with a visibility-restore snapshot in between. Cover a single read and a split across reads. write_filter.rs around line 556 steps over `ESC [` as a complete escape, so `ESC[6` may be written to the ring while the live client's parser aborted that CSI at the switch's ESC. If the test reproduces a query or a parse-state difference that the raw stream does not produce, this feature fixes it: replaying the ring must never leave the client inside a CSI that the live parser closed at a cut. If it does not reproduce, the test is kept and the path is recorded as not reproducing. The outcome is recorded separately from the three stable_id verdicts. Basis: answer requirement.scope.adjacent-gaps (include-adjacent).
- **FR5:** Conditional: a switch sequence in the designator slot misread by extraction (adjacent path). Create-plan first adds a term_core-based regression test for `ESC (` / `ESC )` directly followed by a 47/1047/1049 h/l sequence, both within one read and with `ESC (` ending the previous read. term_core consumes that ESC as the designator and prints the rest as text. `extract_main_buffer_bytes` removes it as a screen switch, which also drives the cut derivation and the live spans. The test compares snapshot + replacement + following chunks against the raw-stream reference: screen, cursor, responses and displayed characters. If it reproduces a difference, this feature fixes it so that main-span extraction and cut derivation agree with term_core on a designator-slot ESC. If it does not reproduce, the test is kept and the path is recorded as not reproducing. The outcome is recorded separately from the three stable_id verdicts. Basis: answer requirement.scope.adjacent-gaps (include-adjacent).
- **FR6:** Record the decisions. Put a decision table under `feature-docs/mux-suppressed-output-round4-fixes/`. It has one row per stable_id: 3e2024dce619ed9f->FR1, 989ec5c588abce06->FR2, 48caec6f5b0b5810->FR3. Each row gives the verdict (resolved / no change needed), the rationale and the matching regression test. A separate section, outside the stable_id table, records the outcome for the two adjacent paths (FR4, FR5): reproduced and fixed, or not reproducing, each with its regression test. Tests whose expectation changes on purpose are listed with old name, new name, file and reason. `feature-docs/mux-suppressed-output-round3-fixes/reviews/round1.yaml` is not modified.

### Non-Functional Requirements

- **NFR1 - Compatibility:** The `mux_ipc` wire format, the Snapshot / SnapshotRestore frame shape, the snapshot byte assembly layouts, and delivery of the replacement as ordinary `PtyOutput` chunks are unchanged. The ring content changes only in these stated ways: (a) a chain closed by a cut, or by the round3 FR4 fallback closing, is not written from its chain head on, and a chain is held across reads until it settles or is closed (FR1); (b) one ESC is written after a waiting `ESC (` / `ESC )` at a cut or fallback closing (FR3); (c) any change FR4/FR5 makes if those paths reproduce.
- **NFR2 - Lock discipline:** The lock order stays `output_target` -> capture exclusion -> ring / shadow parser, and `output_target` -> boundary exclusion. No blocking wait runs while `output_target` or the boundary exclusion is held. Replacement scanning (including FR2's second reading) runs outside the capture and boundary exclusions.
- **NFR3 - Reader hot-path load:** No new pass over the bytes is added to the reader's normal path. Chain-head and construct-start tracking happen inside the existing `scan_boundary` pass. The awaiting-designator ESC write is O(1). FR2's second reading runs only for suppressed chunks under the as-05 fallback condition, and stays a bounded linear scan.
- **NFR4 - Security (TM-1):** Never fabricate a query or viewer launch from a position where the client parser does not start a control sequence, whether through ring replay, replacement items or the re-delivered tail. FR1, FR2 and FR3 (and FR4/FR5 if they reproduce) fix violations of this rule.
- **NFR5 - Security (TM-2):** The write filter's boundary scan, the strip, the client-parity scan (including FR2's two readings) and main-span extraction are bounded and never panic. The 512 KiB `pending` cap and the strip-filtered overflow flush stay in place; a held chain counts toward the cap. An empty replacement is never sent as an empty `PtyOutput` chunk.
- **NFR6 - Build and platforms:** `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml --no-default-features` passes. Behavior is the same on Linux and Windows.

## Assumptions

| ID | Assumption | Reason | Impact | Reversible | Related questions |
|----|------------|--------|--------|------------|-------------------|
| as-01 | The round3 invariants hold: lock order, no blocking wait under `output_target`, the `mux_ipc` wire format, the 512 KiB pending cap, the 256-byte retention window, the replacement delivery order, and term_core's transition rules as the client-parity basis. | Carried over from feature-docs/mux-suppressed-output-round3-fixes/SPEC.md as-01. | medium | yes | - |
| as-02 | FR2 uses the two-reading intersection (an item or tail is kept only when both readings of the chunk's first byte agree). | This is the suggestion in review finding 989ec5c588abce06. It can only cause misses, the same direction as round3 FR5. The answers did not select another approach. | low | yes | - |
| as-03 | A chain is the run of consecutive constructs each closed by the ESC that opens the next one: a string aborted by `ESC <not \>`, or the superseded ESC of `ESC ESC`. A construct closed by an ESC that starts a complete non-string escape (e.g. `ESC [ ... m`) ends the chain. | These are the forms named in finding 3e2024dce619ed9f and in the hold-chain answer. A complete escape leaves the client in ground, so the bytes before it are settled. | medium | yes | requirement.fr1.cross-call-chain |
| as-04 | Round3 SPEC as-02 / EC-2 are superseded in this feature's SPEC and decision record. `feature-docs/mux-suppressed-output-round3-fixes/SPEC.md` is not edited. | Under `.claude/rules/test-docs-records.md`, a predecessor feature's `feature-docs/` prose is outside the update duty. Round3 kept its predecessor's review record unchanged in the same way. | low | yes | requirement.fr3.awaiting-designator-at-cut |
| as-05 | This feature builds on the tip of em-workflow/mux-suppressed-output-round3-fixes/integration (PR #112, unmerged). | Stated by the orchestrator. The three findings concern round3's code. | medium | yes | - |
| as-06 | Create-plan checks `test-docs/mux-suppressed-output-round2-fixes/task0003.tests.yaml` and `test-docs/mux-suppressed-output-round3-fixes/task0002.tests.yaml` / `task0003.tests.yaml` for every test renamed or inverted by FR1-FR3, and applies the test-docs-records update duty. | Those records list the tests that pin round3 as-02/EC-2, the cut behavior and the as-05 rule. | low | yes | - |

## Implementation Approach

### Architecture

Not applicable as a layered architecture. The design step was skipped: this is
a Rust-only change inside the mux daemon (write filter, client-parity scan,
main-span extraction, suppressed-chunk replacement). No UI, child WebView or
visual surface is touched.

**Components named by the requirements:**
```
Write filter          scan_boundary (chain head, construct start), pending,
                      normal drain (write_filter.rs around line 355), cut,
                      awaiting-designator flag, 512 KiB overflow flush      FR1, FR3, FR4
Reader                round3 FR4 fallback closing (pty_spawn/mod.rs)        FR1, FR3
Suppressed path       pending_after tail re-delivery and excluded pieces
                      (suppressed_output.rs)                                FR1
Client-parity scan    as-05 fallback, first_chunk_byte_may_be_designator    FR2
Main-span extraction  extract_main_buffer_bytes, cut derivation, live spans FR5
Decision record       feature-docs/mux-suppressed-output-round4-fixes/      FR6
```

### Lock Order (NFR2)

```
output_target -> capture exclusion -> ring / shadow parser
output_target -> boundary exclusion
```

### API Design

No external API change. The `mux_ipc` wire format and the Snapshot /
SnapshotRestore frame shape are unchanged (NFR1).

### Database Schema

Not applicable.

### Dependencies

**Internal Dependencies:**
- term_core: its transition rules are the client-parity basis (as-01), and it is the reference parser for the AC-2 comparison and the FR4/FR5 checks.
- mux-suppressed-output-round3-fixes: this feature builds on its integration tip (as-05) and carries over its invariants (as-01).

**External Dependencies:**
- None.

## Declared Change Set

This section states the create-plan derivation instead of a hand-authored
list: the feature-specific paths above are derived at create-plan from
every task's `files` entries in `workflow.yaml`
(`references/phases/create-plan-phase.md`).

Every SPEC declares, by default, the following two workflow-generated
entries in addition to the feature-specific paths above:

- `feature-docs/mux-suppressed-output-round4-fixes/**`
- `test-docs/mux-suppressed-output-round4-fixes/**`

`feature-docs/mux-suppressed-output-round4-fixes/**` covers `REQUIREMENTS.md`, `SPEC.md`,
`IMPLEMENTATION.md`, `workflow.yaml`, `phase-state/`, `tasks/`,
`reviews/roundN.yaml`, `VERIFICATION.md`, `retrospect.yaml`, and the design
artifacts the design step produces. These are generated and owned by the
phase documents and by `references/phase-state.md`; this section cites them
and restates none of their rules.

`test-docs/mux-suppressed-output-round4-fixes/**` covers `test-docs/mux-suppressed-output-round4-fixes/{T}.tests.yaml`, the
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
- [ ] **TS-2** (FR1): Write-filter split invariance and the pending postcondition over a chain corpus: `ESC]11;? ESC]0;x`, `ESC]11;? ESC ESC`, `X ESC ESC`, a DCS aborted by an OSC, an APC aborted by a lone ESC. Covers every split position, byte-at-a-time feeding, and cuts at every position. Bytes from the chain head on never appear in the emitted bytes when a cut closes the chain.
- [ ] **TS-3** (FR1): Carried-over completion with a held chain: the incomplete construct at the construct start completes in the next read and is reported with its own bytes and fed end. An aborted chain head is never reported. Tail re-delivery and excluded pieces for a suppressed chunk whose pending holds a chain: no byte is both an item and part of the tail.
- [ ] **TS-5** (FR2): As-05 fallback window (full window, no decidable ESC, trailing `ESC (`): `ESC ( ESC ( ESC ]11;? BEL` yields no item; `ESC ( ESC ( ESC [` yields no tail; constructs both readings agree on are kept (control against a ground window); windows outside the fallback condition are unaffected.
- [ ] **TS-6** (FR3): One ESC is written after a waiting `ESC (` / `ESC )` at a cut in the same call, at a cut at fed 0 after an earlier read left the wait with empty pending, at a cut after an overflow flush whose run ended waiting, and at the mod.rs fallback closing. Nothing extra is written when no designator is awaited.

### Integration Tests
- [ ] **TS-1** (FR1): Main screen, ring not wrapped. Suppress or snapshot around the three FR1 forms before a 47/1047/1049 h/l switch, then feed BEL, `\` or `]11;? BEL` in a later read. No response occurs, and the result matches the raw-stream reference. Repeat with the chain head emitted by an earlier read.
- [ ] **TS-4** (FR1, FR3): Overflow paths: a chain held near the 512 KiB cap that overflows, then a cut; a flushed run that ends waiting for a designator, then a cut. The filter state after the flush matches its definition, and no query appears in the reference comparison.
- [ ] **TS-7** (FR3): The task example `X ESC ( ESC[?1049h ESC[?1049l ESC ( ESC ]11;? BEL` through snapshot + replacement + following chunks: no OSC 11 response, and term_core's parse of the following bytes and its responses equal the raw-stream reference.
- [ ] **TS-8** (FR4): Conditional path check: `ESC[6` + switch + `n`, in one read and split across reads, with a visibility-restore snapshot in between, compared against the raw-stream reference with term_core.
- [ ] **TS-9** (FR5): Conditional path check: `ESC (` / `ESC )` directly followed by `ESC[?1049h` / `ESC[?1047h` / `ESC[?47h` (and the `l` forms), within one read and with `ESC (` ending the previous read, compared against the raw-stream reference (screen, cursor, responses, displayed characters) with term_core.

### E2E Tests
**Existing E2E tests**: None (no E2E inputs were resolved for this feature)
**Run command**: Not detected

### Edge Cases
- [ ] **EC-1** (FR1): A chain whose head lies in an earlier read is held across reads and dropped from its head when a cut closes it.
- [ ] **EC-2** (FR1): A chain that settles before the cut is written through the strip. The carried-over completion is reported for the construct start, and the construct opened after it (closed by the cut) is not written.
- [ ] **EC-3** (FR1): A chain made of DCS/APC strings (e.g. a DCS aborted by `ESC ]`) is dropped from its head, so a later ST cannot complete the DCS in the ring.
- [ ] **EC-4** (FR3): A complete designation (`ESC ( A`) before a cut writes no extra ESC.
- [ ] **EC-5** (FR3, FR5): When the switch sequence sits in the designator slot, FR3's ESC write prevents fabrication whatever FR5 finds. Any remaining display difference (the switch's text that term_core prints) falls under FR5.
- [ ] **EC-6** (FR1, NFR5): A long run of aborted strings grows the held chain up to the 512 KiB cap and takes the overflow flush. It does not panic, and the post-flush state matches its definition.

### Performance Tests
- [ ] **TS-10** (NFR3, NFR5): Adversarial input finishes within the time budget and does not panic: long aborted-string chains near the cap, long `ESC (` chains in the fallback window with FR2's two readings, and repeated switch sequences straddling reads.

### Build Checks
- [ ] **TS-11** (NFR6): `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml --no-default-features` passes.

## Security Considerations

- **Fabrication (TM-1):** See NFR4. No query or viewer launch is fabricated from a position where the client parser does not start a control sequence, whether through ring replay, replacement items or the re-delivered tail.
- **Resource bounds (TM-2):** See NFR5. The boundary scan, the strip, the client-parity scan (including FR2's two readings) and main-span extraction are bounded and never panic; the 512 KiB `pending` cap and the strip-filtered overflow flush stay in place, and a held chain counts toward the cap.
- **Authentication / Authorization / XSS / SQL Injection / CSRF:** Not applicable.

## Error Handling

No new error codes. Per NFR5, the affected scans never panic, and an empty
replacement is never sent as an empty `PtyOutput` chunk.

## Performance Optimization

See NFR3: no new pass over the bytes on the reader's normal path, chain-head
and construct-start tracking inside the existing `scan_boundary` pass, an O(1)
awaiting-designator ESC write, and FR2's second reading only for suppressed
chunks under the as-05 fallback condition as a bounded linear scan.

## Success Criteria

- [ ] All functional requirements are implemented and tested
- [ ] All test scenarios pass
- [ ] AC-1 through AC-9 are met
- [ ] The decision table records all 3 stable_ids, and the FR4/FR5 outcomes are recorded in a separate section (FR6, AC-9)

## Open Questions

> **Note**: 未解決の要件は workflow.yaml で `status: tbd` として管理されています。
> plan フェーズの実行前に解決してください。

None. Every requirement is resolved.

## References

- Requirements document: `feature-docs/mux-suppressed-output-round4-fixes/REQUIREMENTS.md`
- Predecessor SPEC: `feature-docs/mux-suppressed-output-round3-fixes/SPEC.md`
- Predecessor review round 1: `feature-docs/mux-suppressed-output-round3-fixes/reviews/round1.yaml` on branch `em-workflow/mux-suppressed-output-round3-fixes/integration`
- Test-docs records rule: `.claude/rules/test-docs-records.md`
