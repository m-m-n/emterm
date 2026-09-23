# Verification Document: mux-snapshot-ring-wrap-restore

## Overview

**Feature**: mux-snapshot-ring-wrap-restore / **SPEC.md**: `feature-docs/mux-snapshot-ring-wrap-restore/SPEC.md` / **IMPLEMENTATION.md**: `feature-docs/mux-snapshot-ring-wrap-restore/IMPLEMENTATION.md`

This is the integrated verification of the post-wrap main-buffer snapshot restore. Scenario IDs TS-1 to TS-11 map one-to-one to SPEC.md TS1 to TS11. This plan adds TS-12 to TS-15 so that NFR3 to NFR7 each have an explicit verification item. Review round 1 rework (task0002) adds TS-16 and TS-17 for FR9's pending-wrap and grow-resize cases.

## Build Verification

- Command (workflow.yaml `project.components.rust.build_command`): `bash scripts/fetch-fonts.sh && bun install && bun run build:viewer && bun run build:settings && CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml`
- CLI-only build (NFR5, SPEC AC8): `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml --no-default-features`
- Expected: exit code 0 and no errors for both commands.
- TypeScript component: this feature does not change it. Its bundle build runs as part of the Rust build command above.

## Test Verification

- Command (workflow.yaml `project.components.rust.test_command`): `bash scripts/fetch-fonts.sh && bun install && bun run build:viewer && bun run build:settings && CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib -- --test-threads=1`
- If `crates/term_core` was modified: also run `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path crates/term_core/Cargo.toml`
- Coverage target: no coverage tool is configured. Coverage is judged per scenario: every automated TS below maps to at least one passing test.

### Test Scenarios from SPEC.md

| ID | Scenario | Expected Result | Test Type |
|----|----------|-----------------|-----------|
| TS-1 | Reproduction (SPEC TS1). A top-like stream (header row drawn once, then process-row-only frames) is fed to a small-capacity ring and the shadow parser until the total written exceeds capacity. The snapshot is taken through a production assembly entry whose signature the fix does not change, and replayed at the pane dims | Before the fix, the run fails with the header absent. The failure is recorded in the task0001 test record, and the test commit comes before the first production-code change. After the fix, the identical test passes with the header present | Unit (red → green) |
| TS-2 | Header restore on each assembly path (SPEC TS2). The TS-1 stream on a MuxPane fixture goes through (a) `collect_reattach_data` (visible), (b) `handle_request_pane_snapshot` / the on-demand wrapper, (c) `resume_pane_with_permit` and (d) the `evaluate_output_target` resume branch. A variant has shadow dims that differ from the last ring segment's dims | On every path, each visible row's text equals the shadow parser's row, header included. The decoded segments end with (dump block start, `current_dims`) | Integration |
| TS-3 | apt + resize + wrap (SPEC TS3). Filler bytes go past capacity, then an apt-style stream with a mid-run resize; the ring and the shadow parser are resized at the resize point. A snapshot is taken while apt's scroll region is active, and again after its stop sequence | The visible region equals the shadow parser's screen. The mixed-row count is zero when the shadow parser has none. No dump row is shifted or scrolled out | Integration |
| TS-4 | Continued output after a post-wrap snapshot (SPEC TS4). More top frames, or more apt log lines plus bar redraws under the region, are compared against a reference terminal that received the whole stream | The visible region equals the reference. The scroll region, origin mode and cursor position equal the reference | Integration |
| TS-5 | Replay-left origin mode and saved cursor (SPEC TS5). One stream ends with origin mode on and a narrowed region; a variant ends with a pending DECSC. Both are followed by a DECRC and further output | Dumped rows land at absolute rows. The region, origin mode, cursor and SGR equal the replay-established state. The DECRC restores the application's saved position, as on the reference. No dump block contains DECSC or DECRC | Edge |
| TS-6 | Non-wrapped byte identity (SPEC TS6). Rings with total written below capacity and exactly at capacity go through both wrap-aware builders and all four sites | The bytes and segments are identical to the pre-fix layout, with no dump. Pre-existing layout and omission tests pass unmodified | Integration |
| TS-7 | Wrap-state edges (SPEC TS7): a single write larger than capacity, a write of exactly capacity, cumulative writes crossing capacity, `clear` after a wrap, and the combined read | Larger than capacity: wrapped. Exactly capacity: not wrapped. After `clear`: not wrapped. The flag is consistent with the bytes and segments from the same read | Unit |
| TS-8 | No duplicated history (SPEC TS8). The client's history after a post-wrap snapshot is compared with the history after the same payload truncated at the dump block start | The client scrollback has the same length and content | Integration |
| TS-9 | Alt screen unchanged (SPEC TS9). An alt-screen pane with a wrapped ring and with a non-wrapped ring, on both layouts | The bytes and segments are identical to the pre-fix alt layouts | Integration |
| TS-10 | Wire budget (SPEC TS10). A wrapped main-buffer ring with `MAX_DIM_MARKERS` markers plus one cap eviction is encoded and decoded | The segment count is at most `MAX_SEGMENTS`, and the payload decodes as Structured. The existing oversize refusal and stay-detached tests pass | Edge |
| TS-11 | Manual top check (SPEC TS11) | See Manual Testing | Manual |
| TS-12 | Lock hold (plan-added, NFR3) | The visibility-resume sites compute the dump only for alt-screen or wrapped panes. The scrollback critical section of `handle_request_pane_snapshot` holds only the copy (bytes, segments, flag). The probe never runs under the scrollback lock or the shadow-parser lock | Inspection |
| TS-13 | Capacity unchanged (plan-added, NFR4) | A unit test asserts that `DEFAULT_SCROLLBACK_CAPACITY` equals 2 MiB | Unit |
| TS-14 | Build and platforms (plan-added, NFR5, SPEC AC8) | The lib test command passes and the CLI-only check compiles. No platform-specific code is added | Build |
| TS-15 | Known limits (plan-added, NFR6, NFR7) | (a) A unit test shows that a ring rebuilt from a wrapped ring's handoff capture reports not wrapped. (b) The layout SSOT docs state the NFR6 and NFR7 limits | Unit + Inspection |
| TS-16 | Pending wrap after a post-wrap snapshot (rework-added, FR9). The wrapped stream's last output exactly fills a row. Variants: inside a narrowed region with origin mode on; a row ending with a double-width character; a control case where the cursor sits at the last column with no pending wrap. After replay, one more printable character goes to the client and to a whole-stream reference | The client's pending-wrap flag and cursor equal the probe's. After the extra character, the visible rows and cursor equal the reference: it wraps to the next row when a wrap was pending and overwrites the last column in the control case. The dump block has no DECSC, DECRC, ED 3, alt-screen toggle or autowrap change | Edge |
| TS-17 | Probe across grow resizes (rework-added, FR9). A multi-segment pre-dump payload accumulates history, then grows rows only, rows and columns together, and to current_dims larger than the last segment | The probe's scroll region, origin mode, cursor position, SGR and pending-wrap flag equal an oracle with 10,000 history lines replayed on the same payload | Unit |

## Code Quality Verification

- Format: `cargo fmt --manifest-path src-tauri/Cargo.toml --check` (check mode, no rewrite). If `crates/term_core` was modified, also run `cargo fmt --manifest-path crates/term_core/Cargo.toml --check`.
- Static analysis: none is configured for this component.
- Compiler warnings: the changed files introduce no new warnings (for example, an unused entry point).

## SPEC.md Compliance

### Success Criteria

| ID | Criterion | How to Verify |
|----|-----------|---------------|
| AC1 | The reproduction test exists and fails on pre-fix code | TS-1, the task0001 test record, and the commit order (the test commit comes before the first production change) |
| AC2 | The header is restored on all four assembly paths | TS-2 |
| AC3 | Non-wrapped byte identity; existing layout and omission tests unchanged | TS-6 |
| AC4 | Alt-screen byte identity | TS-9 |
| AC5 | apt + resize + wrap replays with zero mixed rows | TS-3 |
| AC6 | Continued output matches the reference terminal | TS-4, TS-5, TS-16 |
| AC7 | No duplicated history | TS-8 |
| AC8 | The lib tests pass and the CLI-only check compiles | TS-14 |
| AC9 | Manual top check | TS-11 |

### Functional Requirements Coverage

| Requirement | Tasks | Verification |
|-------------|-------|--------------|
| FR1 | task0001 | TS-1 |
| FR2 | task0001, task0002 | TS-2, TS-3, TS-8, TS-11 |
| FR3 | task0001 | TS-6 |
| FR4 | task0001 | TS-9 |
| FR5 | task0001 | TS-2 |
| FR6 | task0001 | TS-2, TS-7 |
| FR7 | task0001, task0002 | TS-2, TS-3 |
| FR8 | task0001, task0002 | TS-3, TS-5 |
| FR9 | task0001, task0002 | TS-4, TS-5, TS-16, TS-17 |
| FR10 | task0001, task0002 | TS-1, TS-2, TS-3, TS-4, TS-6 |
| NFR1 | task0001 | TS-6 |
| NFR2 | task0001 | TS-10 |
| NFR3 | task0001 | TS-12 |
| NFR4 | task0001 | TS-13 |
| NFR5 | task0001 | TS-14 |
| NFR6 | task0001 | TS-15 |
| NFR7 | task0001 | TS-15 |

## E2E Testing

Not applicable. The project has no E2E framework configured (`e2e_test_command` is empty for both components).

## Manual Testing (E2E Not Possible)

- [ ] TS-11 (user-run):
  1. Build the release binary with `make build`.
  2. Run `top` in a mux tab for at least 13 minutes. A wider window shortens this.
  3. Switch to another tab and back.
  4. Check that the column header row is still shown.

## Performance / Security Verification (if applicable)

- NFR3 (lock hold): TS-12.
- NFR2 (wire limits): TS-10.
- Robustness: when the daemon-side probe fails, the snapshot degrades to the non-wrapped layout and nothing panics out of assembly (task0001 AC-7(e), unit test through the seam between the probe and assembly).

## Verification Summary

| Category | Items | Automated | E2E | Manual |
|----------|-------|-----------|-----|--------|
| Build | TS-14 | 1 | 0 | 0 |
| Unit | TS-1, TS-7, TS-13, TS-15(a), TS-17 | 5 | 0 | 0 |
| Integration / Edge | TS-2, TS-3, TS-4, TS-5, TS-6, TS-8, TS-9, TS-10, TS-16 | 9 | 0 | 0 |
| Inspection | TS-12, TS-15(b) | 0 | 0 | 2 |
| Manual | TS-11 | 0 | 0 | 1 |
