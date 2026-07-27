//! Snapshot byte assembly helpers shared between the reattach / on-demand
//! snapshot path (`mux::ipc`) and the visibility-resume path
//! (`mux::session::pane`).
//!
//! This module is a leaf inside `crate::mux`: it depends only on
//! `crate::mux::scrollback_filter` and has no back-reference into
//! `mux::ipc` or `mux::session`. Keeping the helpers here breaks the
//! `mux::session::pane` ↔ `mux::ipc::reattach` cycle that would otherwise
//! arise from `pane.rs` importing `build_resume_snapshot_bytes` from
//! `reattach.rs` while `reattach.rs` imports pane types from `pane.rs`.
//!
//! task0004 round-4 rework (D1'): the `scrollback_segments` this module
//! receives (from `ScrollbackRingBuffer::read_segments`) are the SOLE
//! authority for "which dimensions applied to which bytes" — there is no
//! more in-band marker byte for `strip_replayable_rich_content` to exempt.
//! Assembling the snapshot payload strips rich content from `scrollback` as
//! before, but now ALSO remaps each segment's offset past whatever content
//! the strip removed ahead of it (`strip_rich_content_and_remap`), so the
//! caller can carry the returned segments alongside the payload
//! (`mux_ipc::protocol::DimSegment` / `encode_snapshot_payload`) with
//! confidence they still point at the right bytes.

use crate::mux::scrollback_filter::strip_rich_content_and_remap;

/// The clear-and-home prefix every snapshot starts with:
/// `ESC[3J ESC[H ESC[2J`. `ESC[3J` (ED 3) clears the client's existing
/// scrollback so an on-demand snapshot REPLACES the client's history instead
/// of appending to it — the snapshot is ingested via the append-only
/// `PtyOutput` path, so without this each window switch would duplicate the
/// scrollback. `ESC[H ESC[2J` then homes the cursor and clears the screen so
/// the client replays from a known state before the scrollback / screen bytes
/// arrive.
///
/// For `alt_screen == false` (main-buffer panes) the snapshot intentionally
/// relies on scrollback-only reconstruction after this clear: the daemon
/// vt100 `contents_formatted()` dump is omitted (see [`build_snapshot_bytes`])
/// and the client's fresh `term_core` replays the scrollback bytes alone to
/// reconstruct the visible viewport.
const SNAPSHOT_CLEAR_HOME: &[u8] = b"\x1b[3J\x1b[H\x1b[2J";

/// Assemble the shared snapshot byte layout used by the reattach path and
/// the on-demand `RequestPaneSnapshot` path. The layout branches on
/// `alt_screen`:
///
/// ```text
/// alt_screen = false (main-buffer pane):
///     ESC[3J ESC[H ESC[2J + strip(scrollback)          + ESC[?1049l
///
/// alt_screen = true  (alt-screen pane):
///     ESC[3J ESC[H ESC[2J + strip(scrollback) + screen + ESC[?1049h
/// ```
///
/// Rationale for the split:
///
/// - **Main-buffer panes** carry the complete PTY byte history in
///   `scrollback` (the daemon's per-pane ring records every byte the program
///   wrote — including DECSTBM region toggles and progress-bar redraws), so a
///   fresh client `term_core` replays to the correct visible state on its
///   own. The daemon vt100 `contents_formatted()` dump is omitted so the
///   client's view does not pick up trashed cells produced by the daemon's
///   shadow parser — a real-world symptom was apt's progress bar landing on
///   log-line rows after a tab round-trip.
/// - **Alt-screen panes** are the opposite: alt-buffer output is *not*
///   written to scrollback (see `pty_spawn.rs:373`), so the daemon vt100
///   dump is the only source for the visible TUI surface. It is appended
///   after the scrollback so the alt-screen UI paints last, and the trailing
///   `ESC[?1049h` flips the client into alt mode so scrolling/keys behave
///   correctly.
///
/// In both branches the trailing `ESC[?1049{h,l}` normalizes the client's
/// alt-screen flag to the captured pane's actual buffer: `contents_formatted()`
/// never emits the buffer-switch toggle, and the snapshot is applied via the
/// append path (no core reset), so without this the client's alt-screen flag
/// would persist from whatever window it last viewed.
///
/// The `scrollback` bytes are passed through the shared rich-content strip
/// before assembly so a window switch / reattach replays plain-text history
/// without re-spawning rich-content viewers (Markdown / image / JSON / YAML)
/// or re-rendering inline images. The `screen` bytes are
/// `contents_formatted()` (cells only, no viewer launch sequences) so they
/// are passed through unchanged when included.
///
/// `scrollback_segments` is `ScrollbackRingBuffer::read_segments`'s second
/// return value — `(offset, cols, rows)` entries describing which
/// dimensions applied to which bytes of `scrollback`, in ascending offset
/// order. Returns the assembled payload bytes AND the segments re-expressed
/// as offsets into THAT payload (task0004 round-4 rework D1'): the rich-
/// content strip can remove bytes ahead of a segment's original offset, and
/// the clear-prefix / screen / alt-mode assembly around it shifts positions
/// further, so segment offsets computed against the raw scrollback would
/// otherwise silently misalign.
///
/// SSOT: every snapshot-building call site routes through
/// [`build_snapshot_bytes_with_layout`]. The reattach +
/// `RequestPaneSnapshot` path goes via `build_shadow_parser_snapshot`
/// (in `mux::ipc::reattach`) → `build_snapshot_bytes` (this function);
/// the visibility resume path (`resume_pane_with_permit` in
/// `mux::session::pane`) goes via [`build_resume_snapshot_bytes`].
/// They share the strip / main-alt-split logic but differ on the clear
/// prefix and the trailing alt-mode toggle (see `build_resume_snapshot_bytes`).
///
/// `current_dims` (task0005 rework D7'', review round-4 finding
/// `5ba2063e993baf6c`) is the pane's dimensions AT THE MOMENT this snapshot
/// is assembled — the caller's shadow-parser size, which tracks every
/// `MuxPane::resize` exactly. When `screen` is included (`alt_screen ==
/// true`), it is produced under THESE dims, which can differ from the last
/// `scrollback_segments` entry's dims after an `attribute_write` correction
/// (see [`build_snapshot_bytes_with_layout`]'s doc for the scenario). See
/// that function for how this closes the gap.
pub(in crate::mux) fn build_snapshot_bytes(
    scrollback: &[u8],
    scrollback_segments: &[(usize, u16, u16)],
    screen: &[u8],
    alt_screen: bool,
    current_dims: (u16, u16),
) -> (Vec<u8>, Vec<(usize, u16, u16)>) {
    build_snapshot_bytes_with_layout(
        SNAPSHOT_CLEAR_HOME,
        scrollback,
        scrollback_segments,
        screen,
        alt_screen,
        true,
        current_dims,
    )
}

/// Visibility-resume snapshot builder.
///
/// Used by `resume_pane_with_permit` (hidden → visible transition for an
/// already-attached client). Shares the strip + main/alt split with
/// [`build_snapshot_bytes`], but differs in two intentional ways:
///
/// - **Clear prefix is `ESC[H ESC[2J`, not `ESC[3J ESC[H ESC[2J`.** The
///   client is already attached and carries the pane's scrollback in its
///   `term_core`; emitting `ESC[3J` (ED 3) here would wipe that existing
///   history. The reattach path does emit `ESC[3J` because it REPLACES the
///   client's history with the daemon's snapshot of the ring buffer.
/// - **No trailing `ESC[?1049{h,l}` toggle.** Visibility resume preserves
///   the current behavior of not emitting the alt-mode normalization. (If
///   this turns out to be wrong for an alt-screen pane resumed after a
///   visibility hide, fix it as a separate task — out of scope here.)
///
/// Layout:
///
/// ```text
/// alt_screen = false (main-buffer pane):
///     ESC[H ESC[2J + strip(scrollback)
///
/// alt_screen = true  (alt-screen pane):
///     ESC[H ESC[2J + strip(scrollback) + screen
/// ```
pub(in crate::mux) fn build_resume_snapshot_bytes(
    scrollback: &[u8],
    scrollback_segments: &[(usize, u16, u16)],
    screen: &[u8],
    alt_screen: bool,
    current_dims: (u16, u16),
) -> (Vec<u8>, Vec<(usize, u16, u16)>) {
    build_snapshot_bytes_with_layout(
        b"\x1b[H\x1b[2J",
        scrollback,
        scrollback_segments,
        screen,
        alt_screen,
        false,
        current_dims,
    )
}

/// SSOT for snapshot byte assembly. Applies the shared rich-content strip
/// (remapping `scrollback_segments` past whatever it removes) to
/// `scrollback`, includes `screen` only when `alt_screen == true`
/// (main-buffer panes rebuild from scrollback alone — see [`build_snapshot_bytes`]
/// for the rationale), and emits the trailing alt-mode toggle only when
/// `emit_alt_toggle == true`.
///
/// Callers parameterize the clear prefix (`ESC[3J ESC[H ESC[2J` for reattach,
/// `ESC[H ESC[2J` for visibility resume) and the toggle flag; the
/// strip / split logic is shared.
///
/// Segment offset assembly: the FIRST returned segment (if any) always
/// starts at position 0 — covering `clear_prefix` itself, which has no
/// dimension-dependent effect (`ESC[3J`/`ESC[H`/`ESC[2J` behave identically
/// regardless of grid size) — rather than at `clear_prefix.len()`. Every
/// SUBSEQUENT scrollback-derived segment is shifted forward by
/// `clear_prefix.len()`.
///
/// **Trailing screen dump segment (task0005 rework D7'', review round-4
/// finding `5ba2063e993baf6c`):** `screen` is produced at `current_dims` —
/// the pane's dimensions AT THE MOMENT this snapshot is assembled — which
/// can differ from the LAST `scrollback_segments` entry's dims after an
/// `attribute_write` correction changes it (scenario: a resize records
/// `(100, 40)`, then the reader's stale-chunk correction re-attributes
/// intervening content to the OLD `(80, 24)` dims, leaving the newest
/// recorded segment at `(80, 24)` even though the pane is genuinely at
/// `(100, 40)` by the time this snapshot is built). Without an explicit
/// segment for it, `screen` would silently inherit whatever the last
/// scrollback segment says — potentially the WRONG dims. When `screen` is
/// included (`alt_screen == true` and non-empty) and there is at least one
/// scrollback segment to be consistent with, an extra segment for
/// `current_dims` is appended at the position `screen` starts. This is
/// unconditional (not gated on differing from the last segment's dims):
/// when they DO match, replay's `(self.cols, self.rows) != (seg.cols,
/// seg.rows)` check is a no-op, so no extra reflow is introduced in the
/// steady state — only in the divergent case does it change behavior,
/// which is exactly the point.
fn build_snapshot_bytes_with_layout(
    clear_prefix: &[u8],
    scrollback: &[u8],
    scrollback_segments: &[(usize, u16, u16)],
    screen: &[u8],
    alt_screen: bool,
    emit_alt_toggle: bool,
    current_dims: (u16, u16),
) -> (Vec<u8>, Vec<(usize, u16, u16)>) {
    // D8'' (task0005 rework, review round-4 finding `03c11d98f82dfa1c`):
    // the segment-offset assembly below used to rely on "the first entry
    // of `scrollback_segments`, if any, describes offset 0 of the retained
    // window". D1''''' (round-8 rework, review round-7 finding
    // `01f91fe698ceb287`) relaxes that: when 2+ `dim_markers` entries have
    // ever been evicted by the cap, `ScrollbackRingBuffer::read_segments`
    // now DELIBERATELY leaves a leading gap unattributed (no head segment
    // at all) rather than guess — see that method's doc. What still must
    // hold is the ORDERING invariant: entries are non-decreasing in offset,
    // which is what makes the shift-by-`clear_prefix.len()` below correct
    // regardless of whether the first entry starts at 0.
    debug_assert!(
        scrollback_segments.windows(2).all(|w| w[0].0 <= w[1].0),
        "scrollback_segments must be in non-decreasing offset order per \
         ScrollbackRingBuffer::read_segments' contract"
    );
    let watch_offsets: Vec<usize> = scrollback_segments.iter().map(|&(off, _, _)| off).collect();
    let (scrollback, remapped_offsets) = strip_rich_content_and_remap(scrollback, &watch_offsets);
    let screen_to_include: &[u8] = if alt_screen { screen } else { &[] };
    let alt_mode: &[u8] = if emit_alt_toggle {
        if alt_screen {
            b"\x1b[?1049h"
        } else {
            b"\x1b[?1049l"
        }
    } else {
        &[]
    };
    let mut combined = Vec::with_capacity(
        clear_prefix.len() + scrollback.len() + screen_to_include.len() + alt_mode.len(),
    );
    combined.extend_from_slice(clear_prefix);
    combined.extend_from_slice(&scrollback);
    let screen_pos = combined.len();
    combined.extend_from_slice(screen_to_include);
    combined.extend_from_slice(alt_mode);

    // D1''''' (round-8 rework, review round-7 finding `01f91fe698ceb287`):
    // the FIRST entry only gets folded onto position 0 (covering
    // `clear_prefix`, which has no dimension-dependent effect) when it
    // ACTUALLY describes offset 0 of the retained window — the historical
    // case. When `read_segments` left the leading gap unattributed (2+ cap
    // evictions), the first entry's own ORIGINAL offset is already > 0, so
    // it is shifted by `clear_prefix.len()` exactly like every other entry
    // instead of being forced to 0 — the gap ahead of it (now covering
    // `clear_prefix` too) stays a genuine gap, which
    // `TerminalCore::replay_segments` replays at the caller's target dims.
    let mut combined_segments: Vec<(usize, u16, u16)> = scrollback_segments
        .iter()
        .zip(remapped_offsets.iter())
        .enumerate()
        .map(|(i, (&(orig_off, cols, rows), &remapped))| {
            let offset = if i == 0 && orig_off == 0 {
                0
            } else {
                remapped + clear_prefix.len()
            };
            (offset, cols, rows)
        })
        .collect();

    // D7'' trailing screen dump segment: only when `screen` is actually
    // included AND there is at least one scrollback segment to be
    // consistent with (an empty `scrollback_segments` means the caller
    // never tracked dims for this snapshot at all — degrade to the fully
    // legacy no-segments contract instead of introducing a lone segment).
    if !screen_to_include.is_empty() && !combined_segments.is_empty() {
        combined_segments.push((screen_pos, current_dims.0, current_dims.1));
    }

    (combined, combined_segments)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn contains(haystack: &[u8], needle: &[u8]) -> bool {
        !needle.is_empty() && haystack.windows(needle.len()).any(|w| w == needle)
    }

    /// A scrollback that contains an OSC 777 markdown viewer launch must not
    /// appear in the assembled snapshot (it would re-spawn the viewer).
    ///
    /// Driven via the `alt_screen = true` branch so the `screen` slice is
    /// included in the output: the layout-split contract (main-buffer panes
    /// omit the screen slice) is exercised by
    /// `build_snapshot_bytes_main_buffer_omits_screen_part` instead.
    #[test]
    fn build_snapshot_bytes_strips_rich_content_from_scrollback() {
        let scrollback = b"prompt$ \x1b]777;emterm;markdown;begin\x07done";
        let (out, _segments) = build_snapshot_bytes(scrollback, &[], b"SCREEN", true, (80, 24));
        assert!(
            !contains(&out, b"\x1b]777;emterm;markdown"),
            "snapshot must not contain the viewer launch sequence"
        );
        assert!(contains(&out, b"prompt$ "), "plain text history preserved");
        assert!(contains(&out, b"done"), "trailing plain text preserved");
        assert!(contains(&out, b"SCREEN"), "screen contents preserved");
    }

    /// The shared layout helper composes the snapshot byte stream and
    /// branches on `alt_screen`:
    ///
    /// - `alt_screen = false` (main-buffer pane): clear prefix + scrollback
    ///   + `ESC[?1049l`, with NO screen slice. The client rebuilds the
    ///   visible viewport from scrollback alone (FR1).
    /// - `alt_screen = true` (alt-screen pane): clear prefix + scrollback +
    ///   screen + `ESC[?1049h`, identical to the pre-fix layout (FR2).
    ///
    /// Empty-inputs cases stay well-formed in both branches.
    #[test]
    fn build_snapshot_bytes_layout_is_clear_scrollback_screen() {
        // Main-buffer pane (alt_screen = false): screen slice omitted,
        // trailing ESC[?1049l.
        let (out, _) = build_snapshot_bytes(b"SB", &[], b"SC", false, (80, 24));
        assert_eq!(out, b"\x1b[3J\x1b[H\x1b[2JSB\x1b[?1049l");
        // Empty inputs: clear prefix + alt-mode normalization.
        let (empty_out, _) = build_snapshot_bytes(b"", &[], b"", false, (80, 24));
        assert_eq!(empty_out, b"\x1b[3J\x1b[H\x1b[2J\x1b[?1049l");
        // Alt-screen pane (alt_screen = true): screen slice included,
        // trailing ESC[?1049h.
        let (out_alt, _) = build_snapshot_bytes(b"SB", &[], b"SC", true, (80, 24));
        assert_eq!(out_alt, b"\x1b[3J\x1b[H\x1b[2JSBSC\x1b[?1049h");
        let (empty_alt, _) = build_snapshot_bytes(b"", &[], b"", true, (80, 24));
        assert_eq!(empty_alt, b"\x1b[3J\x1b[H\x1b[2J\x1b[?1049h");
    }

    /// FR1 (main-buffer snapshot omits screen dump): for `alt_screen = false`
    /// the returned bytes are exactly
    /// `SNAPSHOT_CLEAR_HOME + stripped_scrollback + ESC[?1049l`, and the
    /// supplied `screen` slice must NOT appear anywhere in the output.
    #[test]
    fn build_snapshot_bytes_main_buffer_omits_screen_part() {
        let scrollback = b"history-line";
        let screen = b"SCREEN-SHOULD-BE-ABSENT";
        let (out, _) = build_snapshot_bytes(scrollback, &[], screen, false, (80, 24));

        // Screen slice must NOT appear anywhere in the output.
        assert!(
            !contains(&out, screen),
            "main-buffer snapshot must not contain the supplied screen slice"
        );

        // The output must be exactly clear + scrollback + ESC[?1049l.
        let mut expected = Vec::new();
        expected.extend_from_slice(b"\x1b[3J\x1b[H\x1b[2J");
        expected.extend_from_slice(scrollback);
        expected.extend_from_slice(b"\x1b[?1049l");
        assert_eq!(out, expected);
    }

    /// task0004 round-4 rework (D1'): a segment describing the whole
    /// scrollback (offset 0) is reported at offset 0 in the combined
    /// output too — the clear prefix is folded into it (position 0, not
    /// `clear_prefix.len()`) since the prefix has no dimension-dependent
    /// effect.
    #[test]
    fn build_snapshot_bytes_head_segment_covers_the_clear_prefix() {
        let scrollback = b"history";
        let segments = [(0usize, 80u16, 24u16)];
        let (out, combined_segments) =
            build_snapshot_bytes(scrollback, &segments, b"", false, (80, 24));
        assert_eq!(combined_segments, vec![(0usize, 80u16, 24u16)]);
        assert!(contains(&out, b"history"));
    }

    /// A mid-scrollback segment (a resize recorded partway through the
    /// retained history) is shifted forward by exactly `clear_prefix.len()`
    /// in the combined output — no strip removed anything ahead of it here.
    #[test]
    fn build_snapshot_bytes_shifts_mid_scrollback_segment_by_clear_prefix_len() {
        let scrollback = b"before-resizeafter-resize";
        let segments = [
            (0usize, 80u16, 24u16),
            ("before-resize".len(), 120u16, 40u16),
        ];
        let (out, combined_segments) =
            build_snapshot_bytes(scrollback, &segments, b"", false, (80, 24));
        let clear_prefix_len = b"\x1b[3J\x1b[H\x1b[2J".len();
        assert_eq!(
            combined_segments,
            vec![
                (0usize, 80u16, 24u16),
                (clear_prefix_len + "before-resize".len(), 120u16, 40u16),
            ]
        );
        assert!(contains(&out, b"before-resize"));
        assert!(contains(&out, b"after-resize"));
    }

    /// AC-3 (round-8 rework, review round-7 finding `01f91fe698ceb287`): a
    /// `scrollback_segments` list whose FIRST entry is NOT at offset 0 (the
    /// shape `ScrollbackRingBuffer::read_segments` now produces when 2+
    /// `dim_markers` entries have been evicted by the cap, leaving the
    /// leading gap deliberately unattributed) must not panic, and must NOT
    /// be forced to position 0 — it is shifted by `clear_prefix.len()`
    /// exactly like any other entry, leaving the gap ahead of it (now
    /// covering `clear_prefix` too) genuinely unattributed for
    /// `TerminalCore::replay_segments` to replay at the caller's target
    /// dims.
    ///
    /// Confirmed to fail pre-fix: the old unconditional `if i == 0 { 0 }`
    /// branch forced this entry to position 0, discarding its true (later)
    /// offset — the assertion below (expecting the shifted, non-zero
    /// position) would fail against that behavior.
    #[test]
    fn build_snapshot_bytes_handles_a_segment_list_whose_first_entry_is_not_at_offset_zero() {
        let scrollback = b"unattributed-gapafter-first-segment";
        let first_offset = "unattributed-gap".len();
        let segments = [(first_offset, 100u16, 30u16)];
        let (out, combined_segments) =
            build_snapshot_bytes(scrollback, &segments, b"", false, (80, 24));
        let clear_prefix_len = b"\x1b[3J\x1b[H\x1b[2J".len();
        assert_eq!(
            combined_segments,
            vec![(clear_prefix_len + first_offset, 100u16, 30u16)],
            "a non-zero-leading first entry must be shifted like any other \
             entry, not forced to position 0"
        );
        assert!(contains(&out, b"unattributed-gap"));
        assert!(contains(&out, b"after-first-segment"));
    }

    /// A segment recorded AFTER a rich-content sequence that gets stripped
    /// must be remapped past the removed bytes, not left pointing at its
    /// original (pre-strip) offset.
    #[test]
    fn build_snapshot_bytes_remaps_segment_past_stripped_rich_content() {
        let mut scrollback = b"before".to_vec();
        let viewer_launch = b"\x1b]777;emterm;markdown;begin\x07";
        scrollback.extend_from_slice(viewer_launch);
        scrollback.extend_from_slice(b"after-resize-content");
        let resize_offset = scrollback.len() - b"after-resize-content".len();
        let segments = [(0usize, 80u16, 24u16), (resize_offset, 120u16, 40u16)];
        let (out, combined_segments) =
            build_snapshot_bytes(&scrollback, &segments, b"", false, (80, 24));
        let clear_prefix_len = b"\x1b[3J\x1b[H\x1b[2J".len();
        assert_eq!(
            combined_segments,
            vec![
                (0usize, 80u16, 24u16),
                (clear_prefix_len + "before".len(), 120u16, 40u16),
            ],
            "the second segment's offset must be remapped past the removed \
             viewer-launch sequence, not left at its pre-strip position"
        );
        assert!(!contains(&out, b"777;emterm;markdown"));
        assert!(contains(&out, b"after-resize-content"));
    }

    /// AC-11 (task0005 rework D7'', review round-4 finding
    /// `5ba2063e993baf6c`): when `screen` is included (alt-screen pane), an
    /// explicit segment for `current_dims` is appended at the position
    /// `screen` starts — even when it differs from the LAST scrollback
    /// segment's dims (the attribution-correction scenario the finding
    /// describes: a resize recorded new dims, then a stale-chunk correction
    /// re-attributed intervening content back to the OLD dims, leaving the
    /// last recorded scrollback segment stale relative to the pane's actual
    /// current size).
    ///
    /// Confirmed to fail pre-fix: before this fix, `screen` had NO segment
    /// of its own — a caller feeding `combined_segments` into
    /// `reset_and_replay_segments` would replay `screen`'s bytes under
    /// whatever the LAST scrollback segment says (80, 24 here), not the
    /// `current_dims` (100, 40) it was actually produced at — this test's
    /// `combined_segments` assertion would then only contain the single
    /// `(0, 80, 24)` entry, missing the trailing `(screen_pos, 100, 40)`
    /// one.
    #[test]
    fn build_snapshot_bytes_appends_a_screen_segment_at_current_dims_even_when_stale_relative_to_last_scrollback_segment()
     {
        let scrollback = b"history";
        // The last (only) recorded scrollback segment is STALE (80, 24) —
        // simulating the attribution-correction scenario where the pane's
        // actual current size has since moved on to `current_dims`.
        let segments = [(0usize, 80u16, 24u16)];
        let screen = b"SCREEN-CONTENT";
        let current_dims = (100u16, 40u16);
        let (out, combined_segments) =
            build_snapshot_bytes(scrollback, &segments, screen, true, current_dims);

        let clear_prefix_len = b"\x1b[3J\x1b[H\x1b[2J".len();
        let screen_pos = clear_prefix_len + scrollback.len();
        assert_eq!(
            combined_segments,
            vec![
                (0usize, 80u16, 24u16),
                (screen_pos, current_dims.0, current_dims.1),
            ],
            "an explicit trailing segment for the screen dump's ACTUAL \
             current dims must be appended, even though it differs from \
             the last recorded scrollback segment"
        );
        assert!(contains(&out, b"SCREEN-CONTENT"));
    }

    /// The trailing screen segment is NOT appended when `scrollback_segments`
    /// is empty (a caller that never tracked dims at all for this
    /// snapshot) — appending a lone segment there would violate the
    /// offset-0 precondition (D8'') and mix a partially-segment-aware
    /// payload with an otherwise fully-legacy one.
    #[test]
    fn build_snapshot_bytes_omits_screen_segment_when_no_scrollback_segments_were_ever_recorded() {
        let (_, combined_segments) = build_snapshot_bytes(b"", &[], b"SCREEN", true, (100, 40));
        assert!(combined_segments.is_empty());
    }

    /// `build_resume_snapshot_bytes` (visibility-resume path) shares the
    /// strip + main/alt split with `build_snapshot_bytes` but uses the
    /// shorter clear prefix `ESC[H ESC[2J` (no `ESC[3J` — the client is
    /// already attached and carries the pane's scrollback in its `term_core`)
    /// and emits NO trailing `ESC[?1049{h,l}` toggle. This pins both the
    /// main-buffer and alt-screen layouts so future drift is caught.
    #[test]
    fn build_resume_snapshot_bytes_layout_main_buffer_and_alt_screen() {
        // Main-buffer pane: screen slice omitted, no trailing toggle.
        assert_eq!(
            build_resume_snapshot_bytes(b"SB", &[], b"SC", false, (80, 24)).0,
            b"\x1b[H\x1b[2JSB",
        );
        // Empty inputs, main-buffer: just the clear prefix.
        assert_eq!(
            build_resume_snapshot_bytes(b"", &[], b"", false, (80, 24)).0,
            b"\x1b[H\x1b[2J",
        );
        // Alt-screen pane: screen slice included, no trailing toggle.
        assert_eq!(
            build_resume_snapshot_bytes(b"SB", &[], b"SC", true, (80, 24)).0,
            b"\x1b[H\x1b[2JSBSC",
        );
        // Empty inputs, alt-screen: just the clear prefix.
        assert_eq!(
            build_resume_snapshot_bytes(b"", &[], b"", true, (80, 24)).0,
            b"\x1b[H\x1b[2J",
        );
    }

    /// `build_resume_snapshot_bytes` must strip rich-content launch
    /// sequences from `scrollback` so a visibility resume does not re-spawn
    /// viewers — same contract as `build_snapshot_bytes`.
    #[test]
    fn build_resume_snapshot_bytes_strips_rich_content_from_scrollback() {
        let scrollback = b"prompt$ \x1b]777;emterm;markdown;begin\x07done";
        let (out, _) = build_resume_snapshot_bytes(scrollback, &[], b"SCREEN", true, (80, 24));
        assert!(
            !contains(&out, b"\x1b]777;emterm;markdown"),
            "resume snapshot must not contain the viewer launch sequence"
        );
        assert!(contains(&out, b"prompt$ "), "plain text history preserved");
        assert!(contains(&out, b"done"), "trailing plain text preserved");
        assert!(contains(&out, b"SCREEN"), "screen contents preserved (alt)");
    }
}
