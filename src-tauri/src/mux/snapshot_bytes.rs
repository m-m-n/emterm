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

use crate::mux::scrollback_filter::strip_replayable_rich_content;

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
/// The `scrollback` bytes are passed through [`strip_replayable_rich_content`]
/// before assembly so a window switch / reattach replays plain-text history
/// without re-spawning rich-content viewers (Markdown / image / JSON / YAML)
/// or re-rendering inline images. The `screen` bytes are
/// `contents_formatted()` (cells only, no viewer launch sequences) so they
/// are passed through unchanged when included.
///
/// SSOT: every snapshot-building call site routes through
/// [`build_snapshot_bytes_with_layout`]. The reattach +
/// `RequestPaneSnapshot` path goes via `build_shadow_parser_snapshot`
/// (in `mux::ipc::reattach`) → `build_snapshot_bytes` (this function);
/// the visibility resume path (`resume_pane_with_permit` in
/// `mux::session::pane`) goes via [`build_resume_snapshot_bytes`].
/// They share the strip / main-alt-split logic but differ on the clear
/// prefix and the trailing alt-mode toggle (see `build_resume_snapshot_bytes`).
pub(in crate::mux) fn build_snapshot_bytes(
    scrollback: &[u8],
    screen: &[u8],
    alt_screen: bool,
) -> Vec<u8> {
    build_snapshot_bytes_with_layout(SNAPSHOT_CLEAR_HOME, scrollback, screen, alt_screen, true)
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
    screen: &[u8],
    alt_screen: bool,
) -> Vec<u8> {
    build_snapshot_bytes_with_layout(b"\x1b[H\x1b[2J", scrollback, screen, alt_screen, false)
}

/// SSOT for snapshot byte assembly. Applies [`strip_replayable_rich_content`]
/// to `scrollback`, includes `screen` only when `alt_screen == true`
/// (main-buffer panes rebuild from scrollback alone — see [`build_snapshot_bytes`]
/// for the rationale), and emits the trailing alt-mode toggle only when
/// `emit_alt_toggle == true`.
///
/// Callers parameterize the clear prefix (`ESC[3J ESC[H ESC[2J` for reattach,
/// `ESC[H ESC[2J` for visibility resume) and the toggle flag; the
/// strip / split logic is shared.
fn build_snapshot_bytes_with_layout(
    clear_prefix: &[u8],
    scrollback: &[u8],
    screen: &[u8],
    alt_screen: bool,
    emit_alt_toggle: bool,
) -> Vec<u8> {
    let scrollback = strip_replayable_rich_content(scrollback);
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
    combined.extend_from_slice(screen_to_include);
    combined.extend_from_slice(alt_mode);
    combined
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
        let out = build_snapshot_bytes(scrollback, b"SCREEN", true);
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
        let out = build_snapshot_bytes(b"SB", b"SC", false);
        assert_eq!(out, b"\x1b[3J\x1b[H\x1b[2JSB\x1b[?1049l");
        // Empty inputs: clear prefix + alt-mode normalization.
        assert_eq!(
            build_snapshot_bytes(b"", b"", false),
            b"\x1b[3J\x1b[H\x1b[2J\x1b[?1049l"
        );
        // Alt-screen pane (alt_screen = true): screen slice included,
        // trailing ESC[?1049h.
        let out_alt = build_snapshot_bytes(b"SB", b"SC", true);
        assert_eq!(out_alt, b"\x1b[3J\x1b[H\x1b[2JSBSC\x1b[?1049h");
        assert_eq!(
            build_snapshot_bytes(b"", b"", true),
            b"\x1b[3J\x1b[H\x1b[2J\x1b[?1049h"
        );
    }

    /// FR1 (main-buffer snapshot omits screen dump): for `alt_screen = false`
    /// the returned bytes are exactly
    /// `SNAPSHOT_CLEAR_HOME + stripped_scrollback + ESC[?1049l`, and the
    /// supplied `screen` slice must NOT appear anywhere in the output.
    #[test]
    fn build_snapshot_bytes_main_buffer_omits_screen_part() {
        let scrollback = b"history-line";
        let screen = b"SCREEN-SHOULD-BE-ABSENT";
        let out = build_snapshot_bytes(scrollback, screen, false);

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
            build_resume_snapshot_bytes(b"SB", b"SC", false),
            b"\x1b[H\x1b[2JSB",
        );
        // Empty inputs, main-buffer: just the clear prefix.
        assert_eq!(
            build_resume_snapshot_bytes(b"", b"", false),
            b"\x1b[H\x1b[2J",
        );
        // Alt-screen pane: screen slice included, no trailing toggle.
        assert_eq!(
            build_resume_snapshot_bytes(b"SB", b"SC", true),
            b"\x1b[H\x1b[2JSBSC",
        );
        // Empty inputs, alt-screen: just the clear prefix.
        assert_eq!(
            build_resume_snapshot_bytes(b"", b"", true),
            b"\x1b[H\x1b[2J",
        );
    }

    /// `build_resume_snapshot_bytes` must strip rich-content launch
    /// sequences from `scrollback` so a visibility resume does not re-spawn
    /// viewers — same contract as `build_snapshot_bytes`.
    #[test]
    fn build_resume_snapshot_bytes_strips_rich_content_from_scrollback() {
        let scrollback = b"prompt$ \x1b]777;emterm;markdown;begin\x07done";
        let out = build_resume_snapshot_bytes(scrollback, b"SCREEN", true);
        assert!(
            !contains(&out, b"\x1b]777;emterm;markdown"),
            "resume snapshot must not contain the viewer launch sequence"
        );
        assert!(contains(&out, b"prompt$ "), "plain text history preserved");
        assert!(contains(&out, b"done"), "trailing plain text preserved");
        assert!(contains(&out, b"SCREEN"), "screen contents preserved (alt)");
    }
}
