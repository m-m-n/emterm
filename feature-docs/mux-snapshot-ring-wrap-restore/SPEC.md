# Feature: mux-snapshot-ring-wrap-restore

## Overview

When a mux main-buffer pane's per-pane scrollback ring has evicted bytes (total_written > capacity), the pane snapshot appends the daemon shadow parser's visible-screen dump after the stripped scrollback replay, so lines written only once (such as top's column header) survive a tab switch, reattach or visibility resume. Non-wrapped main-buffer snapshots and alt-screen snapshots stay byte-identical. Requirements: `feature-docs/mux-snapshot-ring-wrap-restore/REQUIREMENTS.md`.

## Objectives

- After a tab switch, reattach or visibility resume, a mux main-buffer pane shows the same visible content it had before. This includes lines an application wrote only once and never redraws, such as top's column header.
- A regression test catches the loss of a once-written line after the per-pane scrollback ring wraps.

## User Stories

### US1: Once-written lines survive a snapshot after ring wrap
As a mux user, I want a main-buffer pane whose ring has wrapped to show the same visible content after a tab switch, reattach or visibility resume, so that lines written only once, such as top's column header, remain.

**Acceptance Criteria:**
- [ ] AC1: The FR1 reproduction test exists. On pre-fix code it shows the header row missing from the replayed visible region after the ring wraps.
- [ ] AC2: After the fix, the same scenario replays with the visible region equal to the shadow parser's screen, header row included. This holds on each assembly path: visible reattach, on-demand RequestPaneSnapshot, resume_pane_with_permit, and the evaluate_output_target resume branch.
- [ ] AC5: An apt-style stream with a mid-run resize, forced past ring capacity and snapshotted while the scroll region is active, replays with the visible region equal to the shadow parser's screen. The replay has zero rows mixing a bar fragment with a log line when the shadow parser's own screen has none.
- [ ] AC6: After a post-wrap snapshot, further PTY output (top frames; apt log lines plus bar redraws under the active region) yields the same visible region as a reference terminal that received the whole stream.
- [ ] AC7: The client's scrollback history after a post-wrap snapshot gains no duplicated copies of the visible rows because of the dump.
- [ ] AC9 (manual, user-run on a release build): with top running in a mux tab for 13+ minutes (or a wide window), switching tabs away and back keeps the column header.

### US2: Unchanged behaviour outside the post-wrap main-buffer case
As a mux user, I want non-wrapped main-buffer panes and alt-screen panes to keep their current snapshot output, so that the existing protections stay in force.

**Acceptance Criteria:**
- [ ] AC3: For a non-wrapped main-buffer ring (total_written <= capacity), snapshot bytes and segments are byte-identical to the pre-fix output, and existing layout/omission tests pass unchanged.
- [ ] AC4: Alt-screen snapshots are byte-identical to the pre-fix output.
- [ ] AC8: `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib -- --test-threads=1` passes, and `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml --no-default-features` compiles.

## Technical Requirements

### Functional Requirements
- **FR1:** Reproduction test first. Before the fix, add a Rust unit test (inline or sibling tests module) that drives a small-capacity ScrollbackRingBuffer together with a daemon shadow parser (new_shadow_parser), using a top-like stream. The stream writes the header row once by absolute cursor positioning, then repeats frames that update only the process rows until total_written > capacity. The test assembles the snapshot through the production path (read_segments -> build_shadow_parser_snapshot / build_snapshot_bytes). It then replays the result into TerminalCore::reset_and_replay_segments at the pane dims and shows that the header row is missing from the replayed visible region on pre-fix code.
- **FR2:** Post-wrap main-buffer restore (plan A). When a main-buffer pane's ring has evicted bytes (total_written > capacity), the snapshot appends the daemon shadow parser's visible-screen dump (contents_formatted()) after the stripped scrollback replay. As a result, the client's visible region matches the shadow parser's current screen, including rows written only before the eviction point. The scrollback replay still comes first, so client history is rebuilt from the retained ring bytes as today.
- **FR3:** Non-wrapped main-buffer unchanged. For main-buffer panes whose ring has not evicted bytes (total_written <= capacity, including exactly == capacity), snapshot bytes and segments stay byte-identical to the current layout, with no daemon vt100 dump.
- **FR4:** Alt-screen unchanged. Alt-screen pane snapshots (dump + ESC[?1049h on the reattach/on-demand layout, dump without toggle on the resume layout, trailing current_dims segment) are unchanged.
- **FR5:** All assembly sites covered. The post-wrap behaviour applies at every snapshot assembly site: collect_reattach_data (visible reattach), handle_request_pane_snapshot via build_shadow_parser_snapshot (on-demand), resume_pane_with_permit (visibility resume), and the resume branch of evaluate_output_target. Today the two visibility-resume sites skip contents_formatted() for main-buffer panes. They compute it when the ring has wrapped.
- **FR6:** Consistent wrap-state read. ScrollbackRingBuffer exposes whether it has evicted bytes (total_written > capacity). Each assembly site reads that state in the same scrollback lock acquisition as read_segments, so the flag and the bytes describe the same ring state.
- **FR7:** Dump dimension segment. When a main-buffer snapshot includes the dump, the existing trailing (screen_pos, current_dims) segment rule applies, so the dump replays at the pane's current dims. This holds even when the last scrollback segment carries different dims.
- **FR8:** Dump is not distorted by replay-left terminal state. The appended dump draws the shadow parser's screen at the correct rows even when the scrollback replay leaves a scroll region (DECSTBM narrower than the full screen) or origin mode (DECOM) in effect. Each dumped row lands on the same row index as in the shadow parser's screen, and no row is scrolled out or shifted by the dump itself.
- **FR9:** Replay-established state restored after the dump. After the dump is drawn, the scroll region, origin mode and cursor state that the scrollback replay established are in effect again. Cursor state here means position, SGR attributes and the saved-cursor slot. PTY output that continues after the snapshot therefore lands where it would on a terminal that received the whole stream: an apt progress bar pinned below a region, or a top frame updating process rows. The saved-cursor slot left by the replay is neither consumed nor replaced by the snapshot's own additions.
- **FR10:** Regression tests. After the fix, the FR1 test asserts that the header row is present. Screen-comparison tests cover every assembly path, the apt + resize + wrap stream, continued PTY output after a post-wrap snapshot, and non-wrapped byte identity (see Test Scenarios).

### Non-Functional Requirements
- **NFR1 - apt progress-bar debris constraint:** The 22cea6f protection (no daemon vt100 dump in main-buffer snapshots) stays in force for every non-wrapped main-buffer pane. Existing tests that pin the omission for non-wrapped input keep passing unchanged.
- **NFR2 - Wire limits:** Snapshots that carry a dump stay within MAX_SNAPSHOT_FRAME_PAYLOAD, and the existing oversize refusal / stay-detached paths remain intact. They also stay within mux_ipc MAX_SEGMENTS (64): the trailing dump segment uses the slot MAX_DAEMON_SNAPSHOT_SEGMENTS (MAX_DIM_MARKERS + 2) already reserves.
- **NFR3 - Lock hold:** The visibility-resume sites call contents_formatted() only when the dump is included (alt screen, or a wrapped main buffer), so non-wrapped main-buffer panes do not pay for it. handle_request_pane_snapshot keeps its copy-only scrollback critical section.
- **NFR4 - Capacity unchanged:** DEFAULT_SCROLLBACK_CAPACITY stays 2 MiB.
- **NFR5 - Platforms:** Linux and Windows. No platform-specific code is introduced. The --no-default-features (CLI-only) build keeps compiling.
- **NFR6 - Known limit: residual daemon vt100 debris for wrapped panes:** Under plan A, cells already trashed inside the daemon vt100 shadow parser are not repaired. For a pane whose ring has wrapped, the 22cea6f leak path therefore reopens until the ring is cleared (clear()): apt progress-bar fragments on log rows can appear in the restored visible region. This is a known limit, not solved.
- **NFR7 - Known limit: hot-upgrade handoff out of scope:** A hot-upgrade handoff (capture / load_snapshot) keeps only the ring bytes. It loses both the fact that the ring had wrapped and the main-buffer shadow-parser screen. A pane restored from a handoff therefore does not get the post-wrap restore. This is out of scope and a known limit.

## Implementation Approach

### Architecture

**Snapshot layout by pane state:**
```
Pane state                                   Snapshot
-------------------------------------------  -----------------------------------------------
Alt screen (wrapped or not)                  Existing alt layout, unchanged (FR4)
Main buffer, total_written <= capacity       Existing layout, byte-identical, no dump (FR3)
Main buffer, total_written >  capacity       Stripped scrollback replay
                                             + trailing (screen_pos, current_dims) segment
                                               carrying the shadow parser dump (FR2, FR7)
                                             + dump drawn at absolute rows (FR8)
                                             + replay-established region / origin mode /
                                               cursor state restored (FR9)
```

**Component Diagram:**
```
ScrollbackRingBuffer --(wrap state + read_segments, one lock acquisition; FR6)--+
                                                                               |
Daemon shadow parser (vt100) --(contents_formatted(), when wrapped; FR2)-------+
                                                                               v
Assembly sites (FR5):
  - collect_reattach_data (visible reattach)
  - handle_request_pane_snapshot via build_shadow_parser_snapshot (on-demand)
  - resume_pane_with_permit (visibility resume)
  - evaluate_output_target, resume branch
                                                                               |
                                                                               v
Client: TerminalCore::reset_and_replay_segments
```

### Data Flow

```
PTY output -> ScrollbackRingBuffer + daemon shadow parser
Assembly site -> read_segments (+ wrap state) -> build_shadow_parser_snapshot / build_snapshot_bytes
             -> segments (mux_ipc, <= MAX_SEGMENTS) -> client reset_and_replay_segments
```

### API Design

Not applicable. No new wire message is introduced. ScrollbackRingBuffer exposes whether it has evicted bytes (FR6).

### Database Schema

Not applicable.

### Dependencies

**Internal Dependencies:**
- ScrollbackRingBuffer: source of the retained bytes, segments and wrap state (FR6).
- term_core (TerminalCore): client-side replay target; its DECSTBM, DECOM, line-feed and saved-cursor behaviour constrains how the dump is drawn (A4, A5).
- mux_ipc: MAX_SEGMENTS (64) segment budget (NFR2).

**External Dependencies:**
- vt100 0.16.2: daemon shadow parser; contents_formatted() produces the dump (A3). Screen has no public accessor for the scroll region or origin mode (A6).

### File Structure

Derived at create-plan from each task's `files` entries in `workflow.yaml`.

## Declared Change Set

This section states the create-plan derivation instead of a hand-authored
list: the feature-specific paths above are derived at create-plan from
every task's `files` entries in `workflow.yaml`
(`references/phases/create-plan-phase.md`).

Every SPEC declares, by default, the following two workflow-generated
entries in addition to the feature-specific paths above:

- `feature-docs/mux-snapshot-ring-wrap-restore/**`
- `test-docs/mux-snapshot-ring-wrap-restore/**`

`feature-docs/mux-snapshot-ring-wrap-restore/**` covers `REQUIREMENTS.md`, `SPEC.md`,
`IMPLEMENTATION.md`, `workflow.yaml`, `phase-state/`, `tasks/`,
`reviews/roundN.yaml`, `VERIFICATION.md`, `retrospect.yaml`, and the design
artifacts the design step produces. These are generated and owned by the
phase documents and by `references/phase-state.md`; this section cites them
and restates none of their rules.

`test-docs/mux-snapshot-ring-wrap-restore/**` covers `test-docs/mux-snapshot-ring-wrap-restore/{T}.tests.yaml`, the
per-task test record. It is generated and owned by `implement-phase.md`;
this section cites it and restates none of its rules.

These two default entries are part of the declaration unless the SPEC
author explicitly removes them; their absence is never assumed by
silence — removal is a deliberate, explicit narrowing.

This declaration is a SUPERSET assertion: the actual change set observed
at verification time must be CONTAINED IN the declared set, not equal to
it. A feature that produces no implement tasks generates no
`test-docs/mux-snapshot-ring-wrap-restore/` directory at all; the declared
`test-docs/mux-snapshot-ring-wrap-restore/**` entry is still correct in that case — a declared
path that never materializes is not a violation.

## Test Scenarios

### Unit Tests
- [ ] TS1: Reproduction: header lost after wrap (pre-fix) (FR1; AC1). Small-capacity ring + shadow parser fed a top-like stream (header once, then repeated process-row frames) until total_written > capacity. Assemble a snapshot via the production path and replay it into TerminalCore at the pane dims. - Pre-fix: the header row text is absent from the visible region. Post-fix: the same test asserts it is present.
- [ ] TS7: Wrap-state edges (FR6). (a) Single write >= capacity (keep-tail branch). (b) clear() after a wrap. (c) Wrap state read together with read_segments. - (a) counts as wrapped. (b) counts as not wrapped. (c) The flag and bytes describe the same ring state.

### Integration Tests
- [ ] TS2: Screen comparison: header restore after wrap on each assembly path (FR2, FR5, FR6, FR7; AC2). Same stream as TS1 on a MuxPane test fixture. Build the snapshot through (a) collect_reattach_data (visible), (b) handle_request_pane_snapshot / build_shadow_parser_snapshot, (c) resume_pane_with_permit, (d) evaluate_output_target's resume branch. Decode the segments and replay each with reset_and_replay_segments. - For every path, each visible row's text equals the shadow parser's screen row, header included, and the trailing segment carries current_dims.
- [ ] TS3: Screen comparison: apt-style stream + resize + wrap, snapshot mid-run (FR2, FR7, FR8; AC5). Filler bytes past ring capacity, then synth_apt_bytes_with_midrun_resize-style bytes (pty_spawn/tests.rs), with the shadow parser and ring resized at the resize point. Snapshot while apt's DECSTBM region is still active, replay, and count mixed rows with count_mixed_rows. Repeat with the snapshot taken after apt's stop sequence. - Visible region equals the shadow parser's screen, with zero mixed rows when the shadow parser has none. No dump row is shifted or scrolled out by the active region.
- [ ] TS4: Screen comparison: continued PTY output after a post-wrap snapshot (FR9; AC6). After TS2/TS3 replay, feed the next chunk of the same stream (more top frames; more apt log lines + bar redraws under the region) into the replayed client. Feed the entire stream into a fresh reference TerminalCore at the same dims. - The client's visible region equals the reference's. The scroll region, origin mode and cursor position in effect after the snapshot match the reference state.
- [ ] TS6: Non-wrapped byte identity (FR3, NFR1; AC3). Ring with total_written < capacity and with total_written == capacity. Build snapshots through build_snapshot_bytes / build_resume_snapshot_bytes and all four sites. - Bytes and segments are identical to the pre-fix layout (clear prefix + stripped scrollback [+ ESC[?1049l]), with no dump. Existing omission tests still pass.
- [ ] TS8: No duplicated history (AC7). After a post-wrap snapshot replay, compare the client's scrollback length and content with a replay of the same snapshot without the dump. - The dump adds no rows to the client's scrollback history.
- [ ] TS9: Alt-screen unchanged (FR4; AC4). Alt-screen pane with a wrapped and a non-wrapped ring. - Output bytes and segments are identical to the pre-fix alt layout.

### E2E Tests
**Existing E2E tests**: None
**Run command**: Not detected
- [ ] Existing E2E tests pass without regression
- [ ] TS11: Manual top check (user-run) (AC9). Release build; run top in a mux tab for 13+ minutes (or a wide window), switch tabs away and back. - The column header remains.

### Edge Cases
- [ ] TS5: Replay-left origin mode / saved cursor (FR8, FR9). Wrapped-ring stream that ends with DECOM on and a narrowed region, plus a variant that ends with a pending DECSC (saved cursor not yet restored). Snapshot, replay, then feed a DECRC and further output. - The dump lands at absolute rows. DECOM and the region are back in effect afterwards, and the subsequent DECRC restores the application's saved position, matching the reference terminal.
- [ ] TS10: Wire budget (NFR2). Wrapped main-buffer ring with MAX_DIM_MARKERS markers (plus a single cap eviction). Encode and decode the snapshot payload. - Segment count <= MAX_SEGMENTS and the payload decodes as Structured. The oversize refusal path still applies to an oversized payload.

### Performance Tests
- Not applicable. NFR3 (lock hold) and NFR4 (capacity) are structural constraints.

## Security Considerations

Not applicable.

## Error Handling

- Oversized snapshots keep the existing oversize refusal / stay-detached paths (NFR2).

## Performance Optimization

- NFR3: The visibility-resume sites call contents_formatted() only when the dump is included (alt screen, or a wrapped main buffer). handle_request_pane_snapshot keeps its copy-only scrollback critical section.
- NFR4: DEFAULT_SCROLLBACK_CAPACITY stays 2 MiB.

## Assumptions

- A1: Plan A is adopted (requirement.fix-approach answer, batch Codex consultation). The dump is added only for main-buffer panes whose ring has evicted bytes; plan B (always dump) and plan C (larger ring) are rejected.
- A2: The dump is drawn so that a scroll region / origin mode left by the scrollback replay does not distort it, and the replay-established region, origin mode and cursor state are restored afterwards (FR8/FR9).
- A3: Verified fact: vt100 0.16.2 contents_formatted() emits cursor visibility, SGR reset, ESC[H ESC[J, then rows joined by CRLF / CUP / cursor-forward, then a final cursor position and SGR. It emits no DECSTBM, DECOM or autowrap state (vt100 grid.rs:217-252, term.rs:9-13, 372-383). In some end-of-row cursor cases it also emits ESC 7 / ESC 8 (grid.rs:439-442).
- A4: Verified fact: in term_core, a line feed at scroll_region_bottom scrolls the region instead of moving down (terminal_core.rs:922-935), and CUP / ESC[H are region-relative under DECOM (csi_cursor.rs:16-30). The FR8 distortion is therefore real whenever the replay ends with a narrowed region or DECOM on. For top (no DECSTBM) it does not trigger.
- A5: Verified fact: term_core DECSTBM homes the cursor (csi_scroll.rs:20-39), and restore_cursor consumes the single saved-cursor slot (terminal_cursor.rs:108-127, take()). Restoring the region therefore has to be followed by re-establishing the cursor, and ESC 7 / ESC 8 used by the snapshot itself would clobber the application's saved slot.
- A6: Verified fact: vt100 0.16.2 Screen has no public accessor for the scroll region or origin mode (public API: size, cursor_position, alternate_screen, hide_cursor, input modes, attrs). How the replay-established region / origin mode is known when restoring it is a planning decision.
- A7: Known limit (NFR6): cells already trashed inside the daemon vt100 are not repaired, so apt debris can reappear for wrapped panes until clear().
- A8: Known limit (NFR7): the hot-upgrade handoff loses the wrap fact and the main-buffer screen; that path is out of scope.
- A9: Existing apt / wrap tests pass an empty screen slice to build_snapshot_bytes, so they do not by themselves demonstrate safety of the dump; TS3/TS4 supply that evidence.

## Success Criteria

- [ ] All functional requirements are implemented and tested
- [ ] All test scenarios pass
- [ ] AC1-AC9 are met
- [ ] Code review is completed

## Open Questions

> **Note**: 未解決の要件は workflow.yaml で `status: tbd` として管理されています。
> plan フェーズの実行前に解決してください。

- None.

## Implementation Phases

### Phase 1: Reproduction
**Goals:** Show the header loss after ring wrap on pre-fix code (FR1).
**Deliverables:**
- FR1 reproduction test

### Phase 2: Fix
**Goals:** Post-wrap main-buffer restore at every assembly site (FR2-FR9).
**Deliverables:**
- Wrap state on ScrollbackRingBuffer read with read_segments (FR6)
- Dump appended for wrapped main-buffer panes at all four assembly sites (FR2, FR5, FR7, FR8, FR9)

### Phase 3: Regression tests
**Goals:** FR10.
**Deliverables:**
- FR1 test asserting the header row is present
- TS2-TS10

## References

- Requirements: `feature-docs/mux-snapshot-ring-wrap-restore/REQUIREMENTS.md`
