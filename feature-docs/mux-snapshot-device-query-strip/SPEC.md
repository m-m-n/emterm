# Feature: mux-snapshot-device-query-strip

## Overview

Strip response-producing CSI device queries (DA1/DA2, DSR, XTWINOPS reports, DECRPM) from mux snapshot bytes at daemon-side assembly time. This stops the GUI's `PtyOutput` parse path from synthesizing stale replies (e.g. `ESC[?65;1;4;22c`) to historic queries baked into scrollback, which currently corrupts the shell prompt with `65;1;4;22c` on every detach → attach.

## Objectives

- Eliminate the 100%-reproducible prompt corruption on reattach of a zsh-prompt tab
- Keep replay fidelity for every non-query sequence (byte-for-byte)
- Cover all snapshot assembly paths through the single existing filter

## User Stories

### US1: Clean reattach
As a mux user, I want detach → attach to restore my prompt without stray `65;1;4;22c` text, so that reattaching is indistinguishable from never having detached.

**Acceptance Criteria:**
- [ ] A scrollback containing `\x1b[c` replays without the GUI generating a DA1 reply
- [ ] All non-query scrollback content replays unchanged

## Technical Requirements

### Functional Requirements

- **FR1:** `strip_replayable_rich_content` (`src-tauri/src/mux/scrollback_filter.rs`) removes complete CSI sequences that produce a device response in term_core, exactly matching the dispatch conditions in `crates/term_core/src/csi_dispatch.rs`:
  - final `n`, no private-parameter prefix, first parameter ∈ {5, 6} (DSR / CPR)
  - final `c`, prefix none or `?` (DA1) or `>` (DA2), any parameters
  - final `t`, no prefix, first parameter ∈ {14, 16, 18} (XTWINOPS size reports)
  - final `p`, `?` private prefix with `$` intermediate (DECRPM, responds even for unknown modes)
- **FR2:** CSI sequences that do NOT produce a response are preserved byte-for-byte. This includes at minimum: `CSI = … c` (tertiary DA), private-prefixed `n` (e.g. `\x1b[?6n`), `n` with first parameter ∉ {5, 6} (e.g. `\x1b[0n`), `t` with first parameter ∉ {14, 16, 18} (e.g. title-stack `\x1b[22t` / `\x1b[23t`), and `p` finals without the `? … $` shape (DECSTR `\x1b[!p`, DECSCL `\x1b["p`). All currently-preserved content (plain text, SGR, cursor motion, OSC titles, fold marks, mode toggles) stays preserved.
- **FR3:** An incomplete CSI (no final byte before end of buffer) is preserved, matching the filter's existing "unterminated sequences are kept" convention.
- **FR4:** C0 control bytes other than ESC embedded inside a stripped CSI body are re-emitted to the output, because term_core's parser executes them without aborting the sequence (see `payload_has_device_query` in `src-tauri/src/tabs.rs` for the precedent).

### Non-Functional Requirements

- **NFR1 - Performance:** The filter remains a single O(n) pass. The existing `#[ignore]` bench (`strip_replayable_rich_content_bench_2mib_plain`, < 30ms per call on a 2 MiB plain payload) must still pass.
- **NFR2 - Compatibility:** `cargo check --no-default-features` (CLI-only build) still compiles.

## Implementation Approach

### Architecture

The change is confined to the daemon-side scrollback filter, which is already the single funnel for snapshot byte assembly:

```
scrollback ring ──┐
                  ├─ build_snapshot_bytes (snapshot_bytes.rs:137)
shadow screen  ───┘        │  applies strip_replayable_rich_content(scrollback)
                           │
        ┌──────────────────┼──────────────────────┐
   reattach path      on-demand snapshot     visibility resume
 (reattach.rs:196)   (RequestPaneSnapshot)   (pty_spawn.rs:279 applies
                                              the filter directly)
```

Extending `strip_replayable_rich_content` therefore covers every snapshot path with one change. The shadow-screen half (`contents_formatted()`) is a vt100 dump and cannot contain queries; only the scrollback half needs filtering.

### Data Flow

```
Before: scrollback [... \x1b[c ...] → snapshot → GUI PtyOutput parse
        → term_core buffers \x1b[?65;1;4;22c → take_response
        → write_device_response → daemon → PTY → zsh prompt corruption

After:  scrollback [... \x1b[c ...] → strip → snapshot (query removed)
        → GUI PtyOutput parse → no response buffered → clean prompt
```

### CSI recognition rules (raw-byte level)

A candidate begins at `ESC [`. The body consists of parameter bytes (`0x30..=0x3F`, which includes digits, `;`, `:` and the private markers `<=>?`) followed by intermediate bytes (`0x20..=0x2F`), terminated by a final byte (`0x40..=0x7E`). C0 controls (`0x00..=0x1A`, `0x1C..=0x1F`) inside the body are skipped (and re-emitted on strip, per FR4); a bare ESC inside the body aborts the candidate (the aborted prefix is preserved and scanning resumes at the ESC).

Strip decision on a complete CSI, mirroring `csi_dispatch.rs`. term_core dispatches on the FIRST collected intermediate (private markers are collected into the same intermediates array, truncated to `MAX_CSI_INTERMEDIATES = 2`), so trailing intermediates beyond the matched ones never prevent a response:

| final | intermediates condition (term_core view) | first param | action |
|-------|------------------------------------------|-------------|--------|
| `n` | none | 5 or 6 | strip |
| `c` | none, or first ∈ {`?`, `>`} (trailing intermediates ignored) | any | strip |
| `t` | none | 14 / 16 / 18 | strip |
| `p` | first = `?` and second = `$` (bytes beyond the first two intermediates ignored, per parser truncation) | any | strip |
| anything else | — | — | keep |

"First param" is the leading decimal run of the parameter section (empty → 0, matching `ParamParser::get_first_or_zero`).

### Dependencies

**Internal Dependencies:**
- `crates/term_core/src/csi_dispatch.rs` / `csi_device.rs`: the response behavior this filter must mirror (read-only reference; not modified)
- `src-tauri/src/mux/snapshot_bytes.rs`, `src-tauri/src/mux/ipc/pty_spawn.rs`: existing callers (not modified)

**External Dependencies:** none.

### File Structure

```
src-tauri/src/mux/
└── scrollback_filter.rs   # extend strip_replayable_rich_content + unit tests
```

## Test Scenarios

### Unit Tests

- [ ] TS-1: `\x1b[c` (DA1, bare) is stripped; surrounding text preserved
- [ ] TS-2: `\x1b[0c` and `\x1b[?1;2c` (DA1 with params / `?` prefix) are stripped
- [ ] TS-3: `\x1b[>c` / `\x1b[>0c` (DA2) are stripped
- [ ] TS-4: `\x1b[5n` and `\x1b[6n` (DSR/CPR) are stripped
- [ ] TS-5: `\x1b[14t`, `\x1b[16t`, `\x1b[18t` (XTWINOPS reports) are stripped
- [ ] TS-6: `\x1b[?2004$p` (DECRPM, known and unknown modes) is stripped
- [ ] TS-7 (keep set): `\x1b[=c`, `\x1b[?6n`, `\x1b[0n`, `\x1b[22t`, `\x1b[23t`, `\x1b[8;24;80t`, `\x1b[!p`, `\x1b["p` are preserved byte-for-byte
- [ ] TS-8: incomplete CSI at end of buffer (`\x1b[6`) is preserved
- [ ] TS-9: C0 inside a stripped query (`\x1b[\x076n`) → query removed, `\x07` re-emitted
- [ ] TS-10: mixed payload (viewer OSC + device queries + plain text + SGR) → only viewer OSC and queries removed
- [ ] TS-11: all existing `strip_*` tests still pass unchanged
- [ ] TS-12: reattach-shaped regression — a payload equal to `build_snapshot_bytes(scrollback_with_da1, screen, false)` contains no byte subsequence matching a device query (guards the snapshot assembly funnel end-to-end)

### Integration Tests

- covered by TS-12 (snapshot assembly funnel); no new integration harness

### E2E Tests

**Existing E2E tests**: Docker E2E suite exists (`docker-compose.e2e.yml`) but is out of scope per user decision (unit tests only)
**Run command**: not run in this feature
- [ ] N/A — manual detach → attach verification deferred to the user

### Edge Cases

- [ ] Empty scrollback → filter returns empty (existing behavior, no regression)
- [ ] Query split by the 8 MiB reattach chunking: not possible — the filter runs before `send_reattach_data` chunking
- [ ] Bare ESC inside a CSI body aborts the candidate; the kept prefix is byte-identical and scanning resumes at the ESC (e.g. `\x1b[2\x1b[6n` keeps `\x1b[2`, strips `\x1b[6n`)

### Performance Tests

- [ ] Existing `#[ignore]` bench `strip_replayable_rich_content_bench_2mib_plain` stays under its 30ms threshold

## Security Considerations

- **Input Validation:** The filter operates on untrusted PTY byte streams; it must never panic on arbitrary bytes (fuzz-shaped unit inputs in the keep-set tests cover malformed CSI)

## Error Handling

No new error paths: the filter is total (any input produces an output; unrecognized or malformed sequences are preserved).

## Performance Optimization

### Performance Goals
- Single O(n) pass over the scrollback buffer (NFR1)

## Success Criteria

- [ ] All functional requirements are implemented and tested
- [ ] All unit test scenarios pass
- [ ] Existing filter tests pass unchanged
- [ ] `cargo check --no-default-features` passes

## Open Questions

- none

## References

- Root-cause analysis: memory `project_mux_reattach_da1_leak.md`
- Response behavior SSOT: `crates/term_core/src/csi_dispatch.rs` (dispatch), `crates/term_core/src/csi_device.rs` (reply formats)
- Prior art: viewer-OSC stripping in `strip_replayable_rich_content` (same module)
