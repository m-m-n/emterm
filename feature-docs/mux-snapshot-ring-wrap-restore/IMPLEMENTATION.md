# Implementation Plan: mux-snapshot-ring-wrap-restore

## Overview

When a mux main-buffer pane's per-pane scrollback ring has evicted bytes, every snapshot assembly site appends a dump block after the unchanged pre-fix layout. The block draws the daemon shadow parser's visible-screen dump under a normalized scroll region and origin mode, then restores the scroll region, origin mode, cursor position and SGR state that the scrollback replay established. Non-wrapped main-buffer snapshots and alt-screen snapshots stay byte-identical.

## Technology Stack

- **Language**: Rust. The work is in the `src-tauri` crate's `crate::mux` daemon modules, which are built only with the GUI feature.
- **vt100 0.16.2** (existing): the daemon shadow parser. Its formatted visible-screen dump is the source of the restored rows.
- **term_core** (existing in-repo workspace crate): used on the daemon side as the replay-state probe, so the probe applies exactly the replay semantics the client applies.
- **mux_ipc** (existing): the `DimSegment` wire budget (`MAX_SEGMENTS` = 64) and the single-frame payload limit.
- **New dependencies**: none. No third-party crate is added, so the project license (MIT) is unaffected.

## Layer Structure

| Layer | Modules | Responsibility |
|---|---|---|
| Ring state | `mux::scrollback_buffer` | Retained bytes, dimension segments, wrap state |
| Assembly sites | `mux::ipc::reattach` (`collect_reattach_data`, on-demand wrapper), `mux::ipc::handlers` (`handle_request_pane_snapshot`), `mux::session::pane::output_target` (`resume_pane_with_permit`, `evaluate_output_target` resume branch) | Read the ring and the shadow parser under their own locks, call the layout SSOT, apply the existing size policy and framing |
| Layout SSOT | `mux::snapshot_bytes` | Byte layout per pane state |
| Dump block | `mux::snapshot_bytes::dump_block` (new, private child module) | Probe the replay-established state; compose the dump block |
| Terminal model | `term_core` (read-only use) | Replay semantics shared with the client |

Allowed dependency direction: sites → `snapshot_bytes` → `dump_block` → `term_core`. `snapshot_bytes` and `dump_block` never import `mux::ipc` or `mux::session`, so the existing leaf rule that breaks the pane ↔ reattach cycle stays in force. The ring never depends on snapshot assembly.

## Shared Components

| Component | Responsibility | Contract (pre/postcondition) | Used by tasks |
|---|---|---|---|
| Ring combined read: `ScrollbackRingBuffer::read_segments_with_wrap_state() → (content bytes, dimension segments, wrapped)` | Expose the wrap state together with the retained bytes (FR6) | Pre: none. Post: the bytes and segments equal what `read_segments` returns for the same ring state. `wrapped` is true iff the content bytes written since construction or the last `clear` exceed capacity. Exactly equal to capacity gives false, a single write larger than capacity gives true, `clear` resets it to false, and a ring rebuilt by `load_snapshot` gives false. | task0001 |
| Wrap-aware reattach / on-demand builder: `build_snapshot_bytes_for_ring(scrollback, segments, screen, alt_screen, ring_wrapped, current_dims) → (payload, payload segments)` | Reattach / on-demand layout SSOT for every pane state | Post when `ring_wrapped` is false or `alt_screen` is true: exactly the output of the existing `build_snapshot_bytes` for the same inputs. Post when `ring_wrapped` is true, `alt_screen` is false, `screen` is non-empty and the probe succeeds: the payload is the existing non-wrapped main-buffer payload (clear prefix + stripped scrollback + trailing main-buffer normalization) followed by the dump block. The segments are the existing segments followed by (dump block start, `current_dims`) when the existing segments are non-empty. Post on probe failure or an empty `screen`: the non-wrapped output. | task0001 |
| Wrap-aware resume builder: `build_resume_snapshot_bytes_for_ring(same inputs)` | Visibility-resume layout SSOT | Same contract as the reattach builder, relative to the existing `build_resume_snapshot_bytes`. There is no trailing normalization toggle, so the block follows the stripped scrollback. | task0001 |
| Wrap-aware on-demand wrapper: `build_shadow_parser_snapshot_for_ring(shadow parser, scrollback, segments, ring_wrapped)` | Read the dump, the alt flag and the current dims under the shadow-parser lock, then delegate to the wrap-aware reattach builder | Pre: the caller read `scrollback`, `segments` and `ring_wrapped` in one scrollback lock acquisition. Post: equals `build_snapshot_bytes_for_ring` over the parser's state. The shadow-parser lock is released before assembly. | task0001 |
| Replay-state probe (`dump_block`) | Determine the state that the client's replay establishes before the dump block (A6 resolution) | Input: the exact payload bytes that precede the dump block, their segments, and `current_dims`. Post: the scroll region, origin mode, cursor position and current SGR attributes equal the state the client's Snapshot replay reaches at the dump block's first byte, after the trailing `current_dims` transition. The probe uses the same `term_core` replay entry point as the client. A `term_core` panic is contained and reported as a probe failure; it never unwinds into the caller. | task0001 |
| Dump block composer (`dump_block`) | Build the block that follows the pre-fix layout | Input: the probe state and the raw shadow dump. Post: (1) the block contains no DECSC / DECRC, no ED 3, no alt-screen toggle and no autowrap mode change. (2) Replaying the pre-dump payload + block at `current_dims` leaves every visible row equal to the shadow parser's row with the same index; no row is scrolled out or shifted. (3) Afterwards the scroll region, origin mode, cursor position and SGR equal the probe state, and the saved-cursor slot equals what the pre-dump replay left. (4) The block adds no rows to the client's scrollback history. | task0001 |
| `term_core` read-only observers | Let the probe and the tests read the scroll region, origin mode, cursor position, current SGR attributes and saved-cursor slot | Added only where no public accessor exists. Pure reads; `term_core` behaviour does not change. | task0001 |

## Conventions

- **Lock discipline**: each site reads the bytes, segments and wrap state through the combined read inside one scrollback lock acquisition. The probe and the composer never run while the scrollback lock or the shadow-parser lock is held. The scrollback lock and the shadow-parser lock are never nested (unchanged).
- **Escape-sequence vocabulary of the snapshot's own additions**: scroll-region set/reset (DECSTBM), origin mode set/reset (DECSET / DECRST 6), absolute cursor positioning (CUP) and SGR only. The additions never emit DECSC / DECRC, ED 3, alt-screen toggles or autowrap (DECAWM) changes.
- **Error handling**: a probe failure degrades that snapshot to the non-wrapped layout (the pre-fix behaviour) and emits one warn-level log line. Nothing panics out of snapshot assembly. The existing oversize refusal and stay-detached behaviour is unchanged.
- **Logging**: use warn level or higher for anything that must survive release builds. Existing log lines keep their fields; adding the wrap flag to an existing line is allowed.
- **Existing entry points are preserved**: `build_snapshot_bytes`, `build_resume_snapshot_bytes` and `build_shadow_parser_snapshot` keep their signatures and behaviour, so pre-existing tests compile and pass unmodified (NFR1). If an existing entry point is left without production callers, it is restricted to test builds rather than deleted.
- **Documentation**: every doc comment that says main-buffer snapshots omit the shadow dump is updated to state the wrapped-ring exception and the NFR6 / NFR7 known limits.
- **Tests**: a screen comparison compares the per-row text of a `term_core` terminal replayed with `reset_and_replay_segments` at the pane dims against the shadow parser's screen rows. New test code lives in sibling test modules, and pre-existing tests are not edited.

## Cross-task Design Decisions

### D1: Wrap trigger and consistent read (FR6, FR3, NFR7)

The trigger is exactly "content bytes written exceed capacity". It resets on `clear` and is false for a ring rebuilt from a handoff capture (the NFR7 known limit). Sites obtain it only through the combined read, so the flag and the bytes always describe one ring state. Affected tasks: task0001.

### D2: Append-only layout for wrapped main-buffer panes (FR2, FR3, FR7)

The wrapped main-buffer snapshot is the complete pre-fix non-wrapped payload followed by the dump block, and the trailing (dump block start, `current_dims`) segment marks the block. In the reattach / on-demand layout, the main-buffer normalization toggle therefore comes before the block.

Rationale: the part before the dump is byte-identical to what the client replays today, so client history is rebuilt from the retained ring exactly as before. The probe's input is also exactly what the client replays before the block. Any state side effect of that part, including the normalization toggle, is therefore captured by the probe instead of being applied after the restore. Affected tasks: task0001.

### D3: The replay-established state comes from a daemon-side `term_core` probe (FR9, A6)

vt100 exposes no scroll region or origin mode (A6), and FR9 targets the state that the replay established on the client. The probe replays the pre-dump payload in a scratch `term_core` terminal, with the client's own replay semantics, and reads the state back.

Rejected alternatives:

- A byte scanner for DECSTBM / DECOM. It diverges from `term_core` semantics on resets and resizes, and it cannot track the cursor without an emulator.
- Client-side state isolation around the dump. It needs a client-side wire-format change, which is outside the daemon-side scope.
- The shadow parser's own state. It is not observable (A6).

The probe runs only for wrapped main-buffer panes. Their ring is full by definition, so the probe's cost is bounded by the ring capacity. Affected tasks: task0001.

### D4: Dump block composition (FR8, FR9)

Processing flow:

1. Normalize: turn origin mode off and reset the scroll region to the full screen. The dump's home-and-clear and its row-to-row line feeds then address absolute rows.
2. Draw the shadow parser's formatted visible-screen dump with every DECSC / DECRC removed. vt100 emits that pair only to park the cursor in an end-of-row position (A3). The cells drawn do not depend on the pair, and step 3 replaces the cursor placement anyway. As a result, the application's saved-cursor slot is never consumed or replaced.
3. Restore from the probe, in this order: scroll region, origin mode, cursor position (region-relative when origin mode is on), then SGR.

Autowrap is left untouched: the dump is drawn under whatever autowrap mode the replay left. This is a known limit for shadow rows marked as soft-wrapped when the replay left autowrap off. Affected tasks: task0001.

### D5: Lock and cost placement (NFR3, FR5)

The visibility-resume sites read the ring (combined read) before the shadow parser. They compute the dump only when the pane is on the alt screen or the ring has wrapped. `handle_request_pane_snapshot` keeps a copy-only scrollback critical section (bytes, segments, flag). At the visibility-resume sites the probe runs inside the existing `output_target` critical section, which keeps the snapshot-send-then-Connected atomicity unchanged. At `collect_reattach_data` it runs where assembly already runs. Affected tasks: task0001.

### D6: Failure containment (robustness)

The probe processes untrusted PTY bytes on the daemon side. A `term_core` panic is caught, and the snapshot falls back to the non-wrapped layout. A hostile stream therefore cannot take down the daemon and every other session. Affected tasks: task0001.

## Risk Assessment

| Risk | Likelihood | Impact | Mitigation |
|---|---|---|---|
| `term_core` erase-from-home pushes the replayed visible rows into history and duplicates them (SPEC AC7) | Medium | Medium | TS-8 guards it. If it happens, the composer replaces the dump's leading clear with an erase that does not feed history |
| Probe replay cost (up to the 2 MiB ring) while the `output_target` or SessionManager lock is held | Medium | Low | Runs for wrapped main-buffer panes only; the probe uses the client's replay entry point |
| The probe and the client reach different states through different replay entry points | Low | Medium | The probe uses the client's entry point; a test asserts that the probe state equals the `reset_and_replay_segments` state |
| A cursor in the pending-wrap state is restored without the pending flag | Low | Low | Accepted edge case: the next character overwrites the last column instead of wrapping |
| The replay left autowrap off while the shadow screen has soft-wrapped rows | Low | Low | Accepted (D4) and recorded in the docs |
| Residual daemon vt100 debris reappears for wrapped panes (NFR6) | Medium | Medium | Known limit, documented |
| A handoff-restored pane does not get the post-wrap restore (NFR7) | Low | Medium | Known limit, pinned by a unit test |
| A MuxPane test fixture without a recorded resize marker produces no trailing segment | Medium | Low | Fixtures record a marker at the pane dims before writing |

## Open Questions

- [ ] (Planner assumption, non-blocking) TS7(a) says "single write >= capacity counts as wrapped", which conflicts with FR3 at exactly capacity. Assumed: a single write strictly larger than capacity counts as wrapped, and a write of exactly capacity does not, because FR3 governs.
- [ ] (Planner assumption, non-blocking) Autowrap and other replay-left modes (character set, insert mode) are not normalized around the dump (D4). FR8 names only the scroll region and origin mode.
- [ ] (Resolved by D3) The SPEC left open how the replay-established region and origin mode are known (A6).
