# Implementation Plan: mux-strip-concat-query-closure

## Overview
The shared strip writes one CSI_CLOSING (DEL) at a removed construct whenever
its written stream is inside a CSI or right after a written lone ESC. The
write filter carries an O(1) classification of an open CSI, so a device query
split across reads is written with CSI_CLOSING in place of its completing
final byte. No join the strip produces can complete an escape or a device
query the raw stream never made. SPEC:
`feature-docs/mux-strip-concat-query-closure/SPEC.md`.

## Technology Stack
- **Language**: Rust, in the existing `src-tauri` crate (mux daemon modules,
  always built, including under `--no-default-features`).
- **Test oracle**: the in-workspace `term_core` crate, fed the raw stream and
  the written ring (SPEC A3). It is already used by the predecessor suites.
- **New dependencies**: none. The project license (MIT) is unaffected.

## Layer Structure

| Layer | File | Responsibility | May depend on |
|---|---|---|---|
| Shared strip | `src-tauri/src/mux/scrollback_filter.rs` | The single strip pass behind every entry point, the written-stream state, the open-CSI classification, the device-query predicates, and the CSI_CLOSING byte | nothing in the IPC layer |
| Write filter | `src-tauri/src/mux/ipc/pty_spawn/write_filter.rs` | Per-pane state across reads (held chain, designator wait, carried open-CSI classification) and cut handling | shared strip |
| Snapshot assembly | `src-tauri/src/mux/snapshot_bytes.rs` | Visibility-resume and reattach layouts and segment offsets, through the remap form | shared strip |

The dependency direction stays IPC → shared strip. The shared strip never
names a write-filter item.

## Shared Components

| Component | Responsibility | Contract (pre/postcondition) | Used by tasks |
|-----------|----------------|------------------------------|---------------|
| Shared strip pass, through all six entry points: `strip_replayable_rich_content`, `strip_pty_output_for_scrollback_write`, `strip_pty_output_for_scrollback_write_with_designator`, `strip_rich_content_and_remap`, `strip_rich_content_and_remap_with_designator`, `strip_pty_output_for_scrollback_write_with_csi_state` | Remove the strip targets, write closings (D1, D2) and report the end state | Pre: input bytes; a pending-designator flag; for the state-reporting form only, the carried open-CSI classification. The flag and an open classification are never given together. Post: (a) the strip-target predicates and the set of kept bytes are unchanged. (b) Closings follow D1 and D2 exactly. (c) For equal start states every entry point returns byte-identical output. (d) The state-reporting form returns the classification of the written stream's open CSI after the pass, or none. It never returns an escape state. (e) Remapped offsets follow D5. (f) One O(n) pass with O(1) extra state, and no panic for any input. | task0001 |
| Open-CSI classification | An O(1) description of the CSI the written stream is inside | It holds the CSI sub-state (entry or parameter, per `csi_step`), the private marker, the first-parameter saturating accumulator together with whether the leading digit run is still being collected, and the first intermediate byte. Only written bytes advance it, through the transitions of `csi_step` and the accumulation that `scan_csi_device_query` uses. The accumulation is defined once and shared, and the scan's results stay unchanged. At a final byte, the decision "answered device query" is `csi_is_device_query`, unchanged. Post: for every prefix that leaves term_core inside a CSI and every next byte, the classification calls that byte the completing final byte of an answered query exactly when term_core answers the raw stream. | task0001 |
| Write filter carried state | Carry the open-CSI classification across reads | The carried CSI field holds the classification, or none. The `csi_phase` test accessor keeps returning its sub-state. The classification is cleared at every cut. After every cut-free call, its sub-state equals term_core's CSI state on the written bytes. No CSI byte is held in pending, and the postcondition of the pending accessor is unchanged. | task0001 |
| CSI_CLOSING | The single closing byte (DEL) | It is defined once, in the shared strip module. The write filter's existing CSI_CLOSING name refers to the same definition, so existing test imports keep working. | task0001 |

## Conventions
- New tests use the name prefix `strip_concat_`. New write-filter and
  reader-level tests live in a new test module of the `pty_spawn` test tree.
  New strip-level tests join the existing `scrollback_filter` test module.
- Comments that explain a new rule cite `mux-strip-concat-query-closure FRn`
  (and `Dn` from this document where useful).
- No log output is added on the per-read path. The overflow warning stays as
  it is.
- Existing tests are not renamed. Changed expectations are updated in place
  and listed in this feature's DECISIONS.md under "Behavior-changing tests"
  (FR8). No predecessor DECISIONS.md or test-docs record is edited.
- No crate-wide formatter run. Format only the files the task touches, or
  none (the project has no format command).

## Cross-task Design Decisions

### D1: Closing at a removed construct (FR1, FR2, FR4, NFR3)
When the strip removes a construct together with its opening ESC, it first
writes exactly one CSI_CLOSING if the written stream at that point is inside
a CSI (entry or parameter sub-state) or in the escape state (right after a
written lone ESC). Removed constructs are OSC 777 viewer launch, OSC 9999
emterm-md, agent-status, Kitty APC, SIXEL DCS and answered CSI device query.
The closing is written before any C0 byte re-emitted from a removed query,
because a C0 byte right after a lone ESC would be consumed as that escape's
final byte instead of executing. After the closing the written stream is in
ground, so further removed constructs in the same run add nothing. In ground,
or inside a kept string body, nothing is written. In term_core, DEL cancels
an open CSI and ends a pending escape without any display, cursor or response
effect. The rule holds in every entry point, so the write path and the
snapshot path stay identical.
Rationale: one byte, already used for cut closings, and placed where the join
happens. No look-ahead and no second pass are needed.

### D2: Split device query completed in a later read (FR3)
A written final byte that completes the CSI as an answered device query,
according to the classification, is replaced one-for-one by CSI_CLOSING. C0
bytes inside the continuation are written as they arrive, once and in order.
Inside one pass, this can only fire for a CSI carried in from an earlier
read: a complete query whose bytes are all in the pass is already removed by
the existing scan. A split CSI that completes as a non-query is written
unchanged. Split and whole-call outputs may differ in bytes. Replay
equivalence is the requirement (SPEC A4).

### D3: State-reporting form signature (FR3, FR5)
Only the state-reporting form changes signature: it takes and returns the
open-CSI classification instead of the bare sub-state. All other entry points
keep their signatures and start in ground (or in the designator wait).

### D4: Write filter and cuts (FR3, FR7)
The write filter passes its carried classification to the state-reporting
form on every path (drain, cut segment, empty segment, overflow flush) and
stores the result. Its cut logic is unchanged: one closing per cut when the
classification is present, never together with the designator ESC, and
cleared afterwards. After a strip-written closing the stream is in ground,
so a following cut writes nothing. The filter keeps holding a lone trailing
ESC and an ESC ESC chain until it settles, so a lone-ESC join is always seen
inside one strip pass (FR4).

### D5: Offset remapping (FR6)
A watch offset at a removed construct's first byte maps to the output
position before the inserted closing. Offsets strictly inside the removed
span, or right after it, map after the closing and any re-emitted C0 bytes.
Remapped offsets stay non-decreasing and within the output. The remap forms
start in ground, so the D2 replacement never occurs in them.

### D6: No closing at the snapshot end (FR2, SPEC A6)
Snapshot assembly adds no closing at its end. A CSI the program genuinely
left open stays open. The reattach trailing `ESC[?1049{h,l}`, the resume
layouts and the wrapped-ring dump block are unchanged. Rings written by a
pre-fix daemon and carried over by hot-upgrade are out of scope.

## Risk Assessment

| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|------------|
| The classification drifts from term_core (marker, intermediate slot, clamp and u8 truncation of the `n` parameter) | Medium | High (a missed query, or a non-query replaced by DEL) | One shared accumulation definition. A parity test over open prefixes and every next byte against term_core (TS-5). |
| D1 changes written bytes that predecessor expectations pin | High (expected) | Low | Update in place and list each change in DECISIONS.md (FR8). Replay checks against term_core stay the oracle. |
| EC-2 depends on `suppressed_output` re-delivery behaving as assumed | Medium | Medium | Cover EC-2 with the reader harness. If it fails, report a plan deviation instead of editing `suppressed_output`. |
| Remap offsets shift by the inserted closing and misalign `snapshot_bytes` segments | Low | Medium | D5, plus segment tests on rings with a construct removed inside an open CSI (TS-7). |

## Open Questions
- [ ] An overflow flush whose run ends in a lone ESC leaves the written
  stream in the escape state while the carried classification is none. A
  `[6n` in the next read then reaches the ring executable. The snapshot-time
  strip still removes it. The case needs a held run over the pending cap. It
  is not addressed by any FR and is left as a residual for DECISIONS.md.
- [ ] Predecessor review finding `a879a02de382209f` concerns the lone-ESC
  mechanism FR4 addresses. SPEC does not ask for a verdict record on it, so
  none is planned.
