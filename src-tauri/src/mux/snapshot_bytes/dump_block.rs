//! Replay-state probe and dump-block composer for a wrapped main-buffer
//! pane's snapshot restore (mux-snapshot-ring-wrap-restore task0001, D3/D4).
//!
//! Private child module of [`crate::mux::snapshot_bytes`]. Leaf: depends
//! only on `term_core` — never `mux::ipc` / `mux::session` — preserving the
//! parent module's dependency-direction rule (IMPLEMENTATION.md "Layer
//! Structure": sites → `snapshot_bytes` → `dump_block` → `term_core`).

use term_core::cell::{
    STYLE_BLINK, STYLE_BOLD, STYLE_DIM, STYLE_HIDDEN, STYLE_ITALIC, STYLE_REVERSE,
    STYLE_STRIKETHROUGH, STYLE_UNDERLINE,
};
use term_core::terminal_core::{MODE_ORIGIN, ReplaySegment, TerminalCore};

/// The replay-established state the composer restores after drawing the
/// dump block (IMPLEMENTATION.md Shared Components: "Replay-state probe").
///
/// `wrap_pending` (D7, task0002, review round 1): the pending-wrap flag is
/// part of the restored cursor state — the absolute CUP the composer
/// issues to restore `cursor_row`/`cursor_col` clears any pending wrap, so
/// without tracking it separately here the composer would have no way to
/// tell the composer's own cursor-restore step to re-establish it
/// afterward.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ProbeState {
    scroll_top: u16,
    scroll_bottom: u16,
    origin_mode: bool,
    cursor_row: u16,
    cursor_col: u16,
    fg: u32,
    bg: u32,
    flags: u16,
    wrap_pending: bool,
}

/// Scratch scrollback depth for the probe terminal (D8, task0002, review
/// round 1). Matches `default_scrollback_lines()`
/// (`crates/app_settings/src/settings.rs`) — the app's default client-side
/// scrollback capacity, and the depth every probe-equality oracle in this
/// feature's own test suite (`wrap_restore_tests.rs`, `pty_spawn/tests.rs`,
/// this file's own tests) is built with.
///
/// task0001 used `0` here ("the scratch terminal's history capacity may be
/// minimal because only mode state is read") — true only when nothing
/// mid-replay ever resizes the scratch terminal. A segment resize that
/// GROWS the screen breaks that assumption two ways:
///
/// 1. `TerminalCore::resize`'s same-width / full-reflow paths pull rows
///    back OUT of `scrollback_slim` to fill the taller viewport
///    (`crates/term_core/src/reflow.rs`'s GROW branch).
/// 2. Independent of any grow, the SAME resize's cursor-row arithmetic is
///    itself computed from `scrollback_slim.len()` (`sc` / `total_lines` /
///    `cursor_abs`), not merely the viewport row count.
///
/// With a ZERO-capacity scratch terminal, `ring_push_blank` never
/// populates `scrollback_slim` at all — its "scrollback disabled" branch
/// (`crates/term_core/src/ring_buffer.rs`) drops every scrolled-off row
/// outright, so `sc` stays `0` through the whole replay no matter how much
/// content actually scrolled. A terminal with a non-trivial capacity
/// computes a DIFFERENT `cursor_abs` at that same resize — and, on a grow,
/// pulls back real rows the zero-capacity probe has none of — reaching a
/// different post-resize cursor row than the client.
///
/// Why `10_000` suffices: eviction is capacity-driven and deterministic —
/// two terminals replaying the IDENTICAL byte stream under the IDENTICAL
/// capacity reach the IDENTICAL `scrollback_slim` state at every point in
/// the replay, regardless of how much content exceeds that capacity (both
/// simply cap out together, in lockstep). Matching the probe's depth to
/// the depth this feature's own tests build their reference/client
/// terminals with is therefore sufficient for probe-client equality on
/// every payload this feature's test suite exercises, including a resize
/// that grows the screen mid-replay (AC-5).
const PROBE_SCROLLBACK_LINES: u32 = 10_000;

/// D3: replay `pre_dump_payload` in a scratch `term_core` terminal built at
/// `current_dims`, through the SAME replay entry point
/// (`reset_and_replay_segments`) the client uses for `Snapshot` frames, then
/// read back the state that replay established. Building the scratch
/// terminal directly at `current_dims` and relying on
/// `reset_and_replay_segments`'s own "resize back to the caller's target
/// dimensions" postcondition covers D3's "apply the transition to
/// `current_dims`" step for free — no separate resize call is needed.
///
/// Returns `None` on a `term_core` panic during replay (D6: failure
/// containment — a hostile PTY stream must never take the daemon down) or
/// on a degenerate `current_dims` (either axis zero, which
/// `TerminalCore::new` is not built to handle). The caller degrades to the
/// non-wrapped layout in either case.
fn probe_replay_state(
    pre_dump_payload: &[u8],
    pre_dump_segments: &[(usize, u16, u16)],
    current_dims: (u16, u16),
) -> Option<ProbeState> {
    let (cols, rows) = current_dims;
    if cols == 0 || rows == 0 {
        return None;
    }
    let replay_segments: Vec<ReplaySegment> = pre_dump_segments
        .iter()
        .map(|&(offset, seg_cols, seg_rows)| ReplaySegment {
            offset: offset as u32,
            cols: seg_cols,
            rows: seg_rows,
        })
        .collect();
    let payload_owned = pre_dump_payload.to_vec();
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || {
        let mut core = TerminalCore::new(cols, rows, PROBE_SCROLLBACK_LINES);
        core.reset_and_replay_segments(&payload_owned, &replay_segments);
        ProbeState {
            scroll_top: core.get_scroll_region_top(),
            scroll_bottom: core.get_scroll_region_bottom(),
            origin_mode: core.get_mode(MODE_ORIGIN),
            cursor_row: core.get_cursor_row(),
            cursor_col: core.get_cursor_col(),
            fg: core.get_cursor_fg(),
            bg: core.get_cursor_bg(),
            flags: core.get_cursor_flags(),
            wrap_pending: core.get_wrap_pending(),
        }
    }));
    match result {
        Ok(state) => Some(state),
        Err(_) => {
            log::warn!(
                "mux snapshot wrap-restore: replay-state probe panicked while \
                 replaying {}B at {cols}x{rows}; falling back to the non-wrapped \
                 layout",
                pre_dump_payload.len()
            );
            None
        }
    }
}

/// Remove every occurrence of the two-byte DECSC (`ESC 7`) / DECRC
/// (`ESC 8`) sequence from `dump`, and nothing else (D4: "Sanitization
/// removes exactly the two-byte DECSC / DECRC sequences and nothing
/// else").
///
/// vt100's `Screen::contents_formatted()` emits this exact pair ONLY to
/// park the cursor in an end-of-row position after drawing a row whose
/// last cell has no contents but does have attributes (A3): it saves the
/// cursor, backspaces, erases one cell, then restores. The cells drawn
/// never depend on the pair, and the composer's own restore step places
/// the cursor explicitly afterward, so stripping it never loses drawn
/// content — and, because it is never emitted at all, the client's own
/// application-level saved-cursor slot is never consumed or replaced by
/// the dump block (D4, AC-5(c)/(d)).
fn strip_decsc_decrc(dump: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(dump.len());
    let mut i = 0;
    while i < dump.len() {
        if dump[i] == 0x1b && i + 1 < dump.len() && (dump[i + 1] == b'7' || dump[i + 1] == b'8') {
            i += 2;
            continue;
        }
        out.push(dump[i]);
        i += 1;
    }
    out
}

/// Decode the `(tag, r, g, b)` quadruplet `TerminalCore::get_cursor_fg` /
/// `get_cursor_bg` pack into a `u32` (tag in the high byte). Mirrors
/// `term_core::cell::PackedColor::to_u32`'s layout; `PackedColor::from_u32`
/// itself is `#[cfg(test)]`-only in `term_core`, so this crate decodes the
/// wire shape directly rather than depending on a test-only helper.
fn decode_packed_color(v: u32) -> (u8, u8, u8, u8) {
    ((v >> 24) as u8, (v >> 16) as u8, (v >> 8) as u8, v as u8)
}

/// Append the SGR parameter(s) for one packed color (foreground or
/// background) to `params`. A `Default` color (`tag == 0`) needs no
/// parameter — the leading `ESC[0m` full reset in
/// [`sgr_restore_bytes`] already establishes it.
fn push_color_params(params: &mut Vec<String>, color: u32, is_fg: bool) {
    let (tag, r, _g, _b) = decode_packed_color(color);
    match tag {
        1 => {
            // Indexed.
            if is_fg {
                if r < 8 {
                    params.push((30 + r as u32).to_string());
                } else if r < 16 {
                    params.push((90 + (r as u32 - 8)).to_string());
                } else {
                    params.push(format!("38;5;{r}"));
                }
            } else if r < 8 {
                params.push((40 + r as u32).to_string());
            } else if r < 16 {
                params.push((100 + (r as u32 - 8)).to_string());
            } else {
                params.push(format!("48;5;{r}"));
            }
        }
        2 => {
            // RGB.
            let (_, r, g, b) = decode_packed_color(color);
            if is_fg {
                params.push(format!("38;2;{r};{g};{b}"));
            } else {
                params.push(format!("48;2;{r};{g};{b}"));
            }
        }
        _ => {} // Default: nothing to add.
    }
}

/// Encode `(fg, bg, flags)` — the packed-color / style-flag representation
/// `TerminalCore::get_cursor_fg` / `get_cursor_bg` / `get_cursor_flags`
/// return — as a single SGR escape sequence. Always starts from `ESC[0m`
/// (full reset) so the restore is never order-dependent on whatever SGR
/// state the dump block's own drawing left active (D4 step 3: "To restore
/// SGR, the composer resets it and then re-applies the probe's current
/// attributes").
fn sgr_restore_bytes(fg: u32, bg: u32, flags: u16) -> Vec<u8> {
    let mut params: Vec<String> = vec!["0".to_string()];
    if flags & STYLE_BOLD != 0 {
        params.push("1".to_string());
    }
    if flags & STYLE_DIM != 0 {
        params.push("2".to_string());
    }
    if flags & STYLE_ITALIC != 0 {
        params.push("3".to_string());
    }
    if flags & STYLE_UNDERLINE != 0 {
        params.push("4".to_string());
    }
    if flags & STYLE_BLINK != 0 {
        params.push("5".to_string());
    }
    if flags & STYLE_REVERSE != 0 {
        params.push("7".to_string());
    }
    if flags & STYLE_HIDDEN != 0 {
        params.push("8".to_string());
    }
    if flags & STYLE_STRIKETHROUGH != 0 {
        params.push("9".to_string());
    }
    push_color_params(&mut params, fg, true);
    push_color_params(&mut params, bg, false);
    format!("\x1b[{}m", params.join(";")).into_bytes()
}

/// D7 (task0002, review round 1): when the probe reports a pending wrap,
/// re-establish it after the cursor restore and before the SGR restore.
/// The absolute CUP the cursor restore just emitted clears any pending
/// wrap ([`compose_wrapped_dump_block`] step 3), so without this the
/// client's next printed character would land on top of the last column
/// instead of wrapping, diverging from a terminal that replayed the whole
/// stream.
///
/// Reads back, from a scratch replay of `stripped_shadow_dump` (exactly
/// what step 2 of the composer just drew — reading from a replay of the
/// SAME bytes means this reflects the dump's actual rendered content, not
/// the probe's own pre-dump-only state, which the ring's eviction can have
/// left incomplete), the character and attributes of the cell — or, for a
/// double-width character, the wide-cell PAIR's base — that ends at the
/// last column of `cursor_row`. It moves the cursor there (`CSI G`,
/// column-only: the row is already `cursor_row` from the CUP the composer
/// just emitted) and re-prints that content with ITS OWN attributes (a
/// self-contained SGR reset + set; the composer's own final SGR restore,
/// emitted right after this returns, supersedes it for anything that
/// follows). Printing a cell that exactly fills the last column makes
/// `term_core`'s own print path set `wrap_pending` again — the same
/// mechanism the original stream used — and changes no visible cell,
/// since the re-printed content is exactly what the dump already drew
/// there.
fn reestablish_pending_wrap_bytes(
    stripped_shadow_dump: &[u8],
    current_dims: (u16, u16),
    cursor_row: u16,
) -> Vec<u8> {
    let (cols, rows) = current_dims;
    let mut scratch = TerminalCore::new(cols, rows, 0);
    scratch.process_pty_data_fully(stripped_shadow_dump);

    let last_col = cols.saturating_sub(1);
    let base_col = if scratch.get_cell_width(last_col, cursor_row) == 0 {
        // The last column holds a wide-pair's spacer (width 0); the base —
        // the cell that must be re-printed to reproduce the pair — sits
        // one column to the left.
        last_col.saturating_sub(1)
    } else {
        last_col
    };
    let ch = scratch.get_cell_char(base_col, cursor_row);
    let fg = scratch.get_cell_fg(base_col, cursor_row);
    let bg = scratch.get_cell_bg(base_col, cursor_row);
    let flags = scratch.get_cell_flags(base_col, cursor_row);

    let mut out = Vec::with_capacity(ch.len() + 32);
    out.extend_from_slice(format!("\x1b[{}G", base_col + 1).as_bytes());
    out.extend_from_slice(&sgr_restore_bytes(fg, bg, flags));
    out.extend_from_slice(ch.as_bytes());
    out
}

/// D4: compose the dump block that follows the pre-fix layout for a
/// wrapped main-buffer pane.
///
/// `pre_dump_payload` / `pre_dump_segments` are the delegated (pre-fix)
/// payload and its segments — exactly what the client replays before this
/// block. `shadow_dump` is the daemon shadow parser's
/// `contents_formatted()` dump. `current_dims` is the pane's dimensions at
/// the moment the snapshot is assembled.
///
/// Processing flow (D4, extended by D7 for the pending-wrap case):
/// 1. Normalize: origin mode off (`CSI ?6l`), scroll region reset to the
///    full screen (`CSI r`) — the dump's absolute-row addressing assumes
///    full-screen coordinates.
/// 2. Draw `shadow_dump` with every DECSC/DECRC pair removed
///    ([`strip_decsc_decrc`]).
/// 3. Restore from the probe, in this order: scroll region, origin mode,
///    cursor position (region-relative when origin mode is on), pending
///    wrap when the probe reports one
///    ([`reestablish_pending_wrap_bytes`]), then SGR.
///
/// Returns `None` when the probe fails ([`probe_replay_state`]) — the
/// caller then falls back to the non-wrapped layout (D6).
pub(super) fn compose_wrapped_dump_block(
    pre_dump_payload: &[u8],
    pre_dump_segments: &[(usize, u16, u16)],
    shadow_dump: &[u8],
    current_dims: (u16, u16),
) -> Option<Vec<u8>> {
    let probe = probe_replay_state(pre_dump_payload, pre_dump_segments, current_dims)?;

    let stripped_dump = strip_decsc_decrc(shadow_dump);
    let mut block = Vec::with_capacity(stripped_dump.len() + 128);
    // 1. Normalize.
    block.extend_from_slice(b"\x1b[?6l");
    block.extend_from_slice(b"\x1b[r");
    // 2. Draw the shadow dump, DECSC/DECRC stripped.
    block.extend_from_slice(&stripped_dump);
    // 3. Restore: scroll region, origin mode, cursor position, pending
    //    wrap (D7), SGR.
    block.extend_from_slice(
        format!("\x1b[{};{}r", probe.scroll_top + 1, probe.scroll_bottom + 1).as_bytes(),
    );
    if probe.origin_mode {
        block.extend_from_slice(b"\x1b[?6h");
        let row_param = probe.cursor_row.saturating_sub(probe.scroll_top) + 1;
        let col_param = probe.cursor_col + 1;
        block.extend_from_slice(format!("\x1b[{row_param};{col_param}H").as_bytes());
    } else {
        block.extend_from_slice(b"\x1b[?6l");
        let row_param = probe.cursor_row + 1;
        let col_param = probe.cursor_col + 1;
        block.extend_from_slice(format!("\x1b[{row_param};{col_param}H").as_bytes());
    }
    if probe.wrap_pending {
        block.extend_from_slice(&reestablish_pending_wrap_bytes(
            &stripped_dump,
            current_dims,
            probe.cursor_row,
        ));
    }
    block.extend_from_slice(&sgr_restore_bytes(probe.fg, probe.bg, probe.flags));
    Some(block)
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── Sanitization ──────────────────────────────────────────────────

    #[test]
    fn strip_decsc_decrc_removes_only_the_two_byte_pairs() {
        let input = b"AB\x1b7CD\x1b8EF";
        assert_eq!(strip_decsc_decrc(input), b"ABCDEF");
    }

    #[test]
    fn strip_decsc_decrc_leaves_other_escape_sequences_untouched() {
        let input = b"\x1b[31mred\x1b[0m";
        assert_eq!(strip_decsc_decrc(input), input);
    }

    #[test]
    fn strip_decsc_decrc_handles_a_trailing_lone_esc_without_panicking() {
        // A lone trailing ESC (no following byte) must not be treated as
        // the start of a pair and must not panic on the bounds check.
        let input = b"AB\x1b";
        assert_eq!(strip_decsc_decrc(input), b"AB\x1b");
    }

    #[test]
    fn strip_decsc_decrc_empty_input_is_empty_output() {
        assert_eq!(strip_decsc_decrc(b""), Vec::<u8>::new());
    }

    // ── Probe equality ────────────────────────────────────────────────

    /// The probe's replay-established state must equal what an
    /// independently-built oracle terminal — replayed via the exact same
    /// `reset_and_replay_segments` entry point on the same pre-dump
    /// payload — reports through the same public accessors.
    #[test]
    fn probe_replay_state_matches_an_independent_reset_and_replay_segments_oracle() {
        let payload = b"\x1b[5;10r\x1b[?6hline one\r\nline two\x1b[1;33mstyled\x1b[0m".to_vec();
        let segments = vec![(0usize, 80u16, 24u16)];
        let current_dims = (80u16, 24u16);

        let probe = probe_replay_state(&payload, &segments, current_dims)
            .expect("probe must succeed on well-formed input");

        let replay_segments: Vec<ReplaySegment> = segments
            .iter()
            .map(|&(offset, cols, rows)| ReplaySegment {
                offset: offset as u32,
                cols,
                rows,
            })
            .collect();
        let mut oracle = TerminalCore::new(current_dims.0, current_dims.1, 0);
        oracle.reset_and_replay_segments(&payload, &replay_segments);

        assert_eq!(probe.scroll_top, oracle.get_scroll_region_top());
        assert_eq!(probe.scroll_bottom, oracle.get_scroll_region_bottom());
        assert_eq!(probe.origin_mode, oracle.get_mode(MODE_ORIGIN));
        assert_eq!(probe.cursor_row, oracle.get_cursor_row());
        assert_eq!(probe.cursor_col, oracle.get_cursor_col());
        assert_eq!(probe.fg, oracle.get_cursor_fg());
        assert_eq!(probe.bg, oracle.get_cursor_bg());
        assert_eq!(probe.flags, oracle.get_cursor_flags());
    }

    /// A minimal (zero-line) scrollback capacity on the probe's scratch
    /// terminal must not change the mode state it reads back (D3: "The
    /// probe-equality test asserts that this choice does not change that
    /// state") — compare against an oracle built with a generous
    /// scrollback depth instead.
    #[test]
    fn probe_scrollback_capacity_choice_does_not_change_the_observed_state() {
        let mut payload = Vec::new();
        for i in 0..200 {
            payload.extend_from_slice(format!("line {i}\r\n").as_bytes());
        }
        payload.extend_from_slice(b"\x1b[3;20r\x1b[?6h\x1b[2;5H\x1b[1;31mtail");
        let current_dims = (80u16, 24u16);

        let probe = probe_replay_state(&payload, &[], current_dims)
            .expect("probe must succeed on well-formed input");

        let mut oracle = TerminalCore::new(current_dims.0, current_dims.1, 10_000);
        oracle.reset_and_replay_segments(&payload, &[]);

        assert_eq!(probe.scroll_top, oracle.get_scroll_region_top());
        assert_eq!(probe.scroll_bottom, oracle.get_scroll_region_bottom());
        assert_eq!(probe.origin_mode, oracle.get_mode(MODE_ORIGIN));
        assert_eq!(probe.cursor_row, oracle.get_cursor_row());
        assert_eq!(probe.cursor_col, oracle.get_cursor_col());
        assert_eq!(probe.fg, oracle.get_cursor_fg());
        assert_eq!(probe.bg, oracle.get_cursor_bg());
        assert_eq!(probe.flags, oracle.get_cursor_flags());
    }

    // ── Fallback ──────────────────────────────────────────────────────

    /// A degenerate `current_dims` (either axis zero) is a probe failure:
    /// `probe_replay_state` returns `None` rather than reaching
    /// `TerminalCore::new`'s zero-dimension precondition.
    #[test]
    fn probe_replay_state_fails_closed_on_a_zero_dimension() {
        assert!(probe_replay_state(b"anything", &[], (0, 24)).is_none());
        assert!(probe_replay_state(b"anything", &[], (80, 0)).is_none());
    }

    /// `compose_wrapped_dump_block` propagates a probe failure as `None`
    /// (the internal seam AC-7(e) exercises at the builder level) rather
    /// than composing a block from a state it could not establish.
    #[test]
    fn compose_wrapped_dump_block_returns_none_when_the_probe_fails() {
        assert!(compose_wrapped_dump_block(b"pre-dump", &[], b"SCREEN", (0, 24)).is_none());
    }

    // ── Composition shape ─────────────────────────────────────────────

    #[test]
    fn compose_wrapped_dump_block_never_contains_decsc_or_decrc() {
        let shadow_dump = b"\x1b[H\x1b[Jhello\x1b7\x1b[10;1H \x1b[K\x1b8world";
        let block = compose_wrapped_dump_block(b"", &[], shadow_dump, (80, 24))
            .expect("probe must succeed on an empty pre-dump payload");
        assert!(!block.windows(2).any(|w| w == b"\x1b7"));
        assert!(!block.windows(2).any(|w| w == b"\x1b8"));
    }

    #[test]
    fn compose_wrapped_dump_block_restores_a_non_default_scroll_region_and_origin_mode() {
        let pre_dump = b"\x1b[5;20r\x1b[?6h".to_vec();
        let block = compose_wrapped_dump_block(&pre_dump, &[], b"DUMP", (80, 24))
            .expect("probe must succeed");
        // Normalize prefix always resets first.
        assert!(block.starts_with(b"\x1b[?6l\x1b[r"));
        // Restore step re-establishes the probed region and origin mode.
        assert!(
            block
                .windows(b"\x1b[5;20r".len())
                .any(|w| w == b"\x1b[5;20r"),
            "expected the restored scroll region CSI in {block:?}"
        );
        assert!(
            block.windows(b"\x1b[?6h".len()).any(|w| w == b"\x1b[?6h"),
            "expected origin mode restored ON in {block:?}"
        );
    }

    // ── D7: pending-wrap re-establishment ───────────────────────────────

    /// When the probe reports a pending wrap, the composed block
    /// re-establishes it: replaying `pre_dump + block` together leaves the
    /// same `wrap_pending` flag and cursor position the pre-dump payload
    /// alone left, the re-printed cell's content is unchanged, and one
    /// more printable character wraps to the next row exactly as it would
    /// on a terminal that received the whole stream.
    #[test]
    fn compose_wrapped_dump_block_reestablishes_a_pending_wrap_reported_by_the_probe() {
        let cols = 10u16;
        let rows = 5u16;
        // Fills the row exactly (no trailing newline): a pending wrap.
        let pre_dump = vec![b'X'; cols as usize];
        let mut shadow_dump = Vec::new();
        shadow_dump.extend_from_slice(b"\x1b[H\x1b[J\x1b[1;1H");
        shadow_dump.extend_from_slice(&vec![b'X'; cols as usize]);

        let probe = probe_replay_state(&pre_dump, &[], (cols, rows))
            .expect("probe must succeed on well-formed input");
        assert!(
            probe.wrap_pending,
            "test prerequisite: filling the row exactly must leave a pending wrap"
        );

        let block = compose_wrapped_dump_block(&pre_dump, &[], &shadow_dump, (cols, rows))
            .expect("probe must succeed");

        // AC-6: the pending-wrap re-establishment bytes must never introduce
        // DECSC/DECRC (the same regression `compose_wrapped_dump_block_never_
        // contains_decsc_or_decrc` pins for the no-pending-wrap case).
        assert!(!block.windows(2).any(|w| w == b"\x1b7"));
        assert!(!block.windows(2).any(|w| w == b"\x1b8"));

        let mut replayed = TerminalCore::new(cols, rows, 0);
        replayed.process_pty_data_fully(&pre_dump);
        replayed.process_pty_data_fully(&block);

        assert!(
            replayed.get_wrap_pending(),
            "the composed block must re-establish the pending wrap the probe reported"
        );
        assert_eq!(replayed.get_cursor_row(), 0);
        assert_eq!(replayed.get_cursor_col(), cols - 1);
        assert_eq!(
            replayed.get_cell_char(cols - 1, 0),
            "X",
            "the re-print must not change the visible cell's content"
        );

        // One more printable character wraps to the next row's first
        // column, matching a reference fed the whole stream (pre_dump +
        // one more char) directly.
        replayed.process_pty_data_fully(b"Y");
        let mut reference = TerminalCore::new(cols, rows, 0);
        reference.process_pty_data_fully(&pre_dump);
        reference.process_pty_data_fully(b"Y");
        assert_eq!(replayed.get_cursor_row(), reference.get_cursor_row());
        assert_eq!(replayed.get_cursor_col(), reference.get_cursor_col());
        assert_eq!(
            replayed.get_line_text(0).trim_end(),
            reference.get_line_text(0).trim_end()
        );
        assert_eq!(
            replayed.get_line_text(1).trim_end(),
            reference.get_line_text(1).trim_end()
        );
    }

    /// When the probe reports NO pending wrap, the composed block is
    /// unchanged from task0001's composition — no `CSI G` (the byte
    /// sequence [`reestablish_pending_wrap_bytes`] alone emits) appears,
    /// and the block equals byte-for-byte what the pre-D7 steps alone
    /// would have produced.
    #[test]
    fn compose_wrapped_dump_block_leaves_the_block_unchanged_when_no_wrap_is_pending() {
        let pre_dump = b"line one\r\n".to_vec();
        let shadow_dump = b"SCREEN".to_vec();
        let current_dims = (80u16, 24u16);

        let probe = probe_replay_state(&pre_dump, &[], current_dims)
            .expect("probe must succeed on well-formed input");
        assert!(
            !probe.wrap_pending,
            "test prerequisite: a payload ending with a newline leaves no pending wrap"
        );

        let block = compose_wrapped_dump_block(&pre_dump, &[], &shadow_dump, current_dims)
            .expect("probe must succeed");

        let mut expected = Vec::new();
        expected.extend_from_slice(b"\x1b[?6l\x1b[r");
        expected.extend_from_slice(&strip_decsc_decrc(&shadow_dump));
        expected.extend_from_slice(
            format!("\x1b[{};{}r", probe.scroll_top + 1, probe.scroll_bottom + 1).as_bytes(),
        );
        expected.extend_from_slice(b"\x1b[?6l");
        expected.extend_from_slice(
            format!("\x1b[{};{}H", probe.cursor_row + 1, probe.cursor_col + 1).as_bytes(),
        );
        expected.extend_from_slice(&sgr_restore_bytes(probe.fg, probe.bg, probe.flags));

        assert_eq!(block, expected);
    }

    // ── D8: probe history depth matches the client's grow-resize behaviour ──

    fn to_replay_segments_d8(tuples: &[(usize, u16, u16)]) -> Vec<ReplaySegment> {
        tuples
            .iter()
            .map(|&(offset, cols, rows)| ReplaySegment {
                offset: offset as u32,
                cols,
                rows,
            })
            .collect()
    }

    /// AC-5(a): a rows-only grow (same width), after enough pre-resize
    /// content that real history has scrolled off the viewport. Run against
    /// task0001's zero-history probe (`PROBE_SCROLLBACK_LINES = 0`) per the
    /// task plan's "red first" instruction: this specific case did NOT
    /// diverge (both zero-history and the 10,000-line oracle land on the
    /// same post-resize cursor row for this two-segment, same-width-resize
    /// shape) — the divergence the plan anticipated shows up instead in
    /// AC-5(c) below (row 23 at zero probe history vs row 39 against the
    /// oracle, confirmed). Per the plan's fallback ("If none reproduces,
    /// record that in the test record; the equality assertion stays"), this
    /// case is kept as a same-shape regression guard rather than dropped.
    #[test]
    fn probe_replay_state_matches_the_oracle_across_a_rows_only_grow_resize() {
        let mut payload = Vec::new();
        for i in 0..60u32 {
            payload.extend_from_slice(format!("line {i}\r\n").as_bytes());
        }
        let resize_offset = payload.len();
        for i in 60..90u32 {
            payload.extend_from_slice(format!("line {i}\r\n").as_bytes());
        }
        let segments = vec![(0usize, 80u16, 24u16), (resize_offset, 80u16, 40u16)];
        let current_dims = (80u16, 40u16);

        let probe = probe_replay_state(&payload, &segments, current_dims)
            .expect("probe must succeed on well-formed input");

        let mut oracle = TerminalCore::new(current_dims.0, current_dims.1, 10_000);
        oracle.reset_and_replay_segments(&payload, &to_replay_segments_d8(&segments));

        assert_eq!(probe.scroll_top, oracle.get_scroll_region_top());
        assert_eq!(probe.scroll_bottom, oracle.get_scroll_region_bottom());
        assert_eq!(probe.origin_mode, oracle.get_mode(MODE_ORIGIN));
        assert_eq!(probe.cursor_row, oracle.get_cursor_row());
        assert_eq!(probe.cursor_col, oracle.get_cursor_col());
        assert_eq!(probe.fg, oracle.get_cursor_fg());
        assert_eq!(probe.bg, oracle.get_cursor_bg());
        assert_eq!(probe.flags, oracle.get_cursor_flags());
        assert_eq!(probe.wrap_pending, oracle.get_wrap_pending());
    }

    /// AC-5(b): rows AND columns grow together in the same segment
    /// transition (exercises `resize_full_reflow`, not the same-width
    /// path AC-5(a) exercises) — same pre-resize scroll-off precondition.
    /// Also run against the zero-history probe per "red first": also did
    /// not diverge for this fixture shape (see AC-5(a)'s doc comment).
    #[test]
    fn probe_replay_state_matches_the_oracle_across_a_rows_and_columns_grow_resize() {
        let mut payload = Vec::new();
        for i in 0..60u32 {
            payload.extend_from_slice(format!("line {i}\r\n").as_bytes());
        }
        let resize_offset = payload.len();
        for i in 60..90u32 {
            payload.extend_from_slice(format!("line {i}\r\n").as_bytes());
        }
        let segments = vec![(0usize, 80u16, 24u16), (resize_offset, 100u16, 40u16)];
        let current_dims = (100u16, 40u16);

        let probe = probe_replay_state(&payload, &segments, current_dims)
            .expect("probe must succeed on well-formed input");

        let mut oracle = TerminalCore::new(current_dims.0, current_dims.1, 10_000);
        oracle.reset_and_replay_segments(&payload, &to_replay_segments_d8(&segments));

        assert_eq!(probe.scroll_top, oracle.get_scroll_region_top());
        assert_eq!(probe.scroll_bottom, oracle.get_scroll_region_bottom());
        assert_eq!(probe.origin_mode, oracle.get_mode(MODE_ORIGIN));
        assert_eq!(probe.cursor_row, oracle.get_cursor_row());
        assert_eq!(probe.cursor_col, oracle.get_cursor_col());
        assert_eq!(probe.fg, oracle.get_cursor_fg());
        assert_eq!(probe.bg, oracle.get_cursor_bg());
        assert_eq!(probe.flags, oracle.get_cursor_flags());
        assert_eq!(probe.wrap_pending, oracle.get_wrap_pending());
    }

    /// AC-5(c): `current_dims` is larger than the LAST segment declares —
    /// the grow happens only via `reset_and_replay_segments`'s own
    /// trailing "resize back to the caller's target" hop, never via an
    /// explicit segment. Confirmed to fail pre-fix (task0001's zero-history
    /// probe, `PROBE_SCROLLBACK_LINES = 0`): with no scrollback capacity,
    /// `scrollback_slim` never accumulates any rows during the pre-resize
    /// replay (`ring_buffer.rs`'s "scrollback disabled" branch), so the
    /// final resize-to-target-dims hop's row-count arithmetic pulls back
    /// zero rows instead of the real ones, landing on cursor row 23 at zero
    /// probe history versus row 39 against the 10,000-line oracle (both
    /// observed directly by temporarily reverting `PROBE_SCROLLBACK_LINES`
    /// to 0 and re-running this test).
    #[test]
    fn probe_replay_state_matches_the_oracle_when_current_dims_exceeds_the_last_segment() {
        let mut payload = Vec::new();
        for i in 0..60u32 {
            payload.extend_from_slice(format!("line {i}\r\n").as_bytes());
        }
        let segments = vec![(0usize, 80u16, 24u16)];
        let current_dims = (80u16, 40u16);

        let probe = probe_replay_state(&payload, &segments, current_dims)
            .expect("probe must succeed on well-formed input");

        let mut oracle = TerminalCore::new(current_dims.0, current_dims.1, 10_000);
        oracle.reset_and_replay_segments(&payload, &to_replay_segments_d8(&segments));

        assert_eq!(probe.scroll_top, oracle.get_scroll_region_top());
        assert_eq!(probe.scroll_bottom, oracle.get_scroll_region_bottom());
        assert_eq!(probe.origin_mode, oracle.get_mode(MODE_ORIGIN));
        assert_eq!(probe.cursor_row, oracle.get_cursor_row());
        assert_eq!(probe.cursor_col, oracle.get_cursor_col());
        assert_eq!(probe.fg, oracle.get_cursor_fg());
        assert_eq!(probe.bg, oracle.get_cursor_bg());
        assert_eq!(probe.flags, oracle.get_cursor_flags());
        assert_eq!(probe.wrap_pending, oracle.get_wrap_pending());
    }
}
