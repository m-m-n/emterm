//! Mode-bit constants and the data types carried alongside
//! [`TerminalCore`](super::TerminalCore): pending prompt / fold marks,
//! structural replay segments, snapshot-replay input, and the
//! saved-cursor state.

use super::*;

// ── Mode bit positions (matches SPEC.md) ─────────────────

pub const MODE_AUTO_WRAP: u8 = 0;
pub const MODE_ORIGIN: u8 = 1;
pub const MODE_CURSOR_VISIBLE: u8 = 2;
pub const MODE_CURSOR_BLINK: u8 = 3;
pub const MODE_REVERSE_SCREEN: u8 = 4;
pub const MODE_BRACKETED_PASTE: u8 = 5;
pub const MODE_FOCUS_TRACKING: u8 = 6;
pub const MODE_COLUMN_132: u8 = 7;
pub const MODE_SYNCHRONIZED_OUTPUT: u8 = 8;
// Bits 9-10: cursor keys (2 bits)
// Bits 11-12: mouse tracking (2 bits)
// Bits 13-14: mouse encoding (2 bits)
/// Alternate-screen flag, set/cleared by the buffer-switch modes
/// (CSI ?47 / ?1047 / ?1049 h/l). Internal bookkeeping so parse-time
/// consumers (OSC 133 prompt-mark capture) can suppress work while a
/// full-screen app owns the display — the WebView build tracks the same
/// state JS-side (`isAlternateBuffer`) and is unaffected by this bit.
pub const MODE_ALT_SCREEN: u8 = 15;
/// DECSET 1007 (alternate_scroll). When set, the host translates wheel
/// events to arrow-key bytes while the alternate screen is active so
/// AltScreen apps (Claude Code, less, vim, ...) scroll their own log
/// instead of moving eMterm's scrollback. Default ON at construction
/// time, matching xterm / WezTerm. The host also gates on its own
/// `alternate_scroll_enabled` user setting before emitting bytes.
pub const MODE_ALTERNATE_SCROLL: u8 = 16;

// ── Pending OSC 133 prompt marks ─────────────────────────

/// Upper bound on `TerminalCore::pending_prompt_marks`. A producer that
/// emits OSC 133 without ever advancing the cursor (no newline) could
/// otherwise grow this buffer without bound — the PTY is a trust
/// boundary. When the cap is hit we drop the oldest pending mark so the
/// buffer stays bounded; the consumer (`take_prompt_marks`) normally
/// drains it every pump, so the cap is only reached under abuse.
pub const MAX_PENDING_PROMPT_MARKS: usize = 4096;

/// An OSC 133 semantic-prompt mark captured at the moment the handler ran,
/// before the consumer (native-poc) has a chance to read the core. The
/// absolute row and the eviction counter are snapshotted here because the
/// frame can shift (scrollback eviction) between the handler firing and
/// the consumer draining; the consumer normalizes `abs_row` against the
/// current eviction total.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PendingPromptMark {
    /// OSC 133 sub-type as a raw byte: `b'A'`/`b'B'`/`b'C'`/`b'D'`. Only
    /// these four are ever pushed (the handler filters unknown kinds).
    pub kind: u8,
    /// Absolute scrollback-frame row at the moment the mark was received:
    /// `scrollback_len + cursor.row`. May need normalization by the
    /// consumer if scrollback evicted rows after this snapshot.
    pub abs_row: u32,
    /// Optional exit code attached to a `D` (CommandEnd) mark.
    pub exit_code: Option<i32>,
    /// `scrollback_evicted_total` at the moment the mark was received.
    /// The consumer uses `current_evicted_total - this` to shift `abs_row`
    /// into the consumer's current frame before storing it.
    pub evicted_total: u64,
}

// ── Pending custom fold marks (OSC 777;emterm;fold) ──────

/// Upper bound on `TerminalCore::pending_fold_marks`. A producer that
/// floods `OSC 777;emterm;fold;begin` without advancing the cursor could
/// otherwise grow this buffer without bound — the PTY is a trust
/// boundary. When the cap is hit the oldest pending mark is dropped.
/// Mirrors [`MAX_PENDING_PROMPT_MARKS`].
pub const MAX_PENDING_FOLD_MARKS: usize = 4096;

/// Whether a captured custom-fold mark is a `begin` or an `end`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FoldMarkKind {
    /// `OSC 777;emterm;fold;begin;<label>` — opens a region.
    Begin,
    /// `OSC 777;emterm;fold;end` — closes the most recent open region.
    End,
}

/// A custom-fold mark (`OSC 777;emterm;fold;begin|end`) captured at the
/// moment the handler ran, mirroring [`PendingPromptMark`] but for the
/// fold pipeline. The absolute row and eviction counter are snapshotted
/// here because the frame can shift (scrollback eviction) between the
/// handler firing and the consumer draining; the native consumer
/// normalizes `abs_row` against the current eviction total. `begin/end`
/// pairing is left entirely to the consumer (native-poc) so `term_core`
/// stays a thin accumulator. Carries an owned `label` (only meaningful
/// for `Begin`), so unlike `PendingPromptMark` it is not `Copy`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingFoldMark {
    /// Whether this mark opened (`begin`) or closed (`end`) a region.
    pub kind: FoldMarkKind,
    /// Absolute scrollback-frame row at the moment the mark was received:
    /// `scrollback_len + cursor.row`. Mirrors the WebView `lineIndex`
    /// (`scrollbackLength + cursor.row`) captured at OSC-receipt time.
    pub abs_row: u32,
    /// `scrollback_evicted_total` at the moment the mark was received.
    /// The consumer uses `current_evicted_total - this` to shift `abs_row`
    /// into its current frame before pairing begin↔end.
    pub evicted_total: u64,
    /// Fold label, carried only on `Begin` marks (empty otherwise). The
    /// `begin` payload is `OSC 777;emterm;fold;begin;<label>`; the consumer
    /// substitutes a `"..."` fallback for an empty label at registration.
    pub label: String,
}

// ── Structural replay segments (task0004 round-4 rework, D1') ───────────

/// A structural dimension segment for [`TerminalCore::reset_and_replay_segments`]
/// / [`TerminalCore::build_from_snapshot`]: content starting at byte `offset`
/// into the replay payload was produced under `(cols, rows)`, until the next
/// segment (if any, in the same slice) takes over.
///
/// Segments must be supplied in ascending `offset` order — the caller's
/// responsibility (mirrors the ordering invariant the daemon-side
/// `ScrollbackRingBuffer::dim_markers` structure already keeps; this module
/// trusts it rather than re-validating).
///
/// Design D1' (mux-render-corruption round-4 rework): dimensions travel
/// HERE, structurally, alongside the payload — never encoded as a
/// recognizable byte sequence inside it. No byte sequence a child process
/// can produce is therefore ever misinterpreted as a dimension change,
/// because nothing scans the payload for one any more — this is the
/// structural replacement for the in-band `OSC 777;emterm;resize;…` marker
/// byte scan rounds 1-3 tried (and repeatedly failed) to filter safely.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReplaySegment {
    pub offset: u32,
    pub cols: u16,
    pub rows: u16,
}

// ── Off-thread snapshot replay result ────────────────────

/// Output of [`TerminalCore::build_from_snapshot`]: a freshly built core
/// plus everything the synchronous `reset_and_replay` + `drain_marks`
/// site would have produced. Returned by value so the whole bundle can be
/// moved from a worker thread to the main thread for the swap + reconcile.
///
/// `core` is `Send` (it is built with no callbacks installed; see the
/// `static_assert_terminal_core_is_send` below), so this struct is `Send`
/// as well.
pub struct SnapshotReplay {
    /// The fully replayed core, sized to the requested grid.
    pub core: TerminalCore,
    /// Mode actions accumulated during the replay (alt-screen reseed input).
    pub actions: Vec<u8>,
    /// `get_scrollback_evicted_total()` immediately after the replay — the
    /// `evicted_baseline` the caller installs (a fresh core's counter is 0,
    /// matching the synchronous `reset_frame_for_replay`).
    pub evicted_total: u64,
    /// Prompt marks drained from the replayed core (OSC 133), for the
    /// caller's `backfill_prompt_marks`.
    pub prompt_marks: Vec<PendingPromptMark>,
    /// Custom-fold marks drained from the replayed core (OSC 777;…;fold),
    /// for the caller's `backfill_fold_marks`.
    pub fold_marks: Vec<PendingFoldMark>,
    /// Pre-captured command-row texts for OSC 133 B marks, keyed by
    /// `abs_row`. Populated by `push_pending_prompt_mark` during the bypass
    /// replay at the moment each B mark is emitted, before the row can
    /// scroll into the discarded virtual scrollback. The consumer
    /// (`tabs.rs::extract_line_text`) should prefer this map over a
    /// scrollback lookup when the scrollback contents are unavailable (i.e.
    /// after a `build_from_snapshot` replay where the bypass was active).
    /// Empty when the replay was not performed via `build_from_snapshot`.
    pub bypass_b_mark_texts: std::collections::HashMap<u32, String>,
    /// Whether `core.scrollback_slim` / `scrollback_wrapped` were actually
    /// populated by this replay (task0004 round-4 rework D3', review
    /// round-3 finding `b235e4dbc61cc4ba`).
    ///
    /// `build_scrollback_only_from_snapshot` (bypass off) always leaves this
    /// `true`. `build_from_snapshot` (bypass on) leaves it `false` in the
    /// common case (contents intentionally not populated — see
    /// `build_from_snapshot`'s doc comment) — EXCEPT when
    /// `build_from_snapshot_inner` downgrades out of the bypass for THIS
    /// payload (a row-count-growing segment transition, D6), in which case
    /// the drain ran fully populated despite going through the
    /// `build_from_snapshot` entry point.
    ///
    /// The consumer (`tabs.rs::apply_offthread_swap`) MUST branch on this
    /// flag rather than unconditionally spawning the 2nd-pass scrollback
    /// restore worker: spawning it after a replay that ALREADY populated
    /// scrollback would re-prepend the same history a second time,
    /// duplicating it up to the ring's full capacity. Before this field
    /// existed, the D6 bypass downgrade silently broke that assumption for
    /// any payload where rows grew within the retained window (a maximized
    /// window / font-size change survived across a reattach or window
    /// switch) — a common, not exotic, sequence.
    pub scrollback_populated: bool,
}

/// Compile-time guarantee that a built `TerminalCore` can be moved across
/// threads. `build_from_snapshot` constructs the core on a worker thread
/// and the result is moved back to the main thread for the swap; if a
/// future field made the core `!Send`, this assertion fails to compile and
/// the off-thread design must be revisited before that field lands.
///
/// The `callbacks` field (`Option<Box<dyn TerminalCallbacks>>`) is `Send`
/// because [`crate::callbacks::TerminalCallbacks`] requires `Send`.
const _: () = {
    const fn static_assert_send<T: Send>() {}
    static_assert_send::<TerminalCore>();
    static_assert_send::<SnapshotReplay>();
};

// ── SlimStats (FR11 debug export) ────────────────────────

/// Compact statistics about the SlimCell scrollback storage.
#[derive(serde::Serialize)]
pub struct SlimStats {
    pub slim_cells: u32,
    pub style_entries: u32,
    pub style_bytes: u32,
    pub char_entries: u32,
    pub char_bytes: u32,
}

// ── CursorState ──────────────────────────────────────────

/// Per-cursor saved state for DECSC/DECRC (`save_cursor` / `restore_cursor`)
/// and the no-saved-state reset path.
///
/// Cursor shape and blink are deliberately NOT fields here (cursor-settings-fix
/// D1): they live at the [`TerminalCore`] level instead
/// (`cursor_style_default` / `cursor_blink_default` / `cursor_style_override`
/// / `cursor_blink_override`), so DECSC/DECRC save/restore and the
/// no-saved-state restore path can never clobber the settings-derived
/// defaults or an active DECSCUSR/OSC 22 override.
#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub(crate) struct CursorState {
    pub(crate) col: u16,
    pub(crate) row: u16,
    pub(crate) fg: PackedColor,
    pub(crate) bg: PackedColor,
    pub(crate) flags: u16,
    pub(crate) visible: bool,
    // SaveCursor/RestoreCursor extended fields
    pub(crate) g0_charset: u8,
    pub(crate) g1_charset: u8,
    pub(crate) origin_mode: bool,
    pub(crate) wrap_pending: bool,
}

impl CursorState {
    pub(crate) fn new() -> Self {
        Self {
            col: 0,
            row: 0,
            fg: PackedColor::DEFAULT,
            bg: PackedColor::DEFAULT,
            flags: 0,
            visible: true,
            g0_charset: 0,
            g1_charset: 0,
            origin_mode: false,
            wrap_pending: false,
        }
    }
}
