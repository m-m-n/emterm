/// TerminalCore: viewport grid and terminal state.
///
/// Owns the viewport grid (rows × cols cells), cursor state, terminal modes,
/// tab stops, and dirty row tracking. Pure Rust API; the wasm/ crate
/// re-exposes this struct through wasm-bindgen for the TypeScript side.
use std::collections::VecDeque;

use crate::callbacks::TerminalCallbacks;
use crate::cell::*;
use crate::char_table::CharTable;
use crate::slim_cell::SlimCell;
use crate::style_table::StyleTable;

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

// ── TerminalCore ─────────────────────────────────────────

pub struct TerminalCore {
    pub(crate) cols: u16,
    pub(crate) rows: u16,
    // Viewport ring buffer: rotates among `rows` viewport rows.
    pub(crate) ring_cells: Vec<Cell>,   // length = rows × cols
    pub(crate) ring_wrapped: Vec<bool>, // length = rows
    pub(crate) ring_head: usize,        // Index of oldest viewport row in ring (0..rows)
    pub(crate) ring_size: usize,        // Always equals `rows` after construction.
    pub(crate) ring_capacity: usize,    // Total target capacity (= rows + scrollback_lines).
    // Scrollback storage (compressed): oldest rows at front, newest at back.
    pub(crate) scrollback_slim: VecDeque<Vec<SlimCell>>,
    pub(crate) scrollback_wrapped: VecDeque<bool>,
    pub(crate) scrollback_capacity: usize, // Maximum number of scrollback rows.
    /// Monotonic count of scrollback rows ever evicted from the *front*
    /// (oldest end) of `scrollback_slim`, across both the automatic
    /// at-capacity eviction in `ring_push_blank` and the explicit
    /// `evict_oldest_scrollback` API. Consumers that track absolute line
    /// coordinates (e.g. native-poc's prompt-mark tracker) read the delta
    /// since their last observation to shift their stored line indices
    /// down. Reset to 0 by `reset()` — see that method for the rationale.
    ///
    /// While the snapshot-replay bypass is on (see
    /// `scrollback_bypass`), this counter is still maintained: once the
    /// virtual scrollback length saturates at `scrollback_capacity`, each
    /// subsequent scroll-off bumps `scrollback_evicted_total` instead of
    /// growing the (virtual) length. This keeps the observable bookkeeping
    /// byte-identical to the live path.
    pub(crate) scrollback_evicted_total: u64,
    /// Replay-mode bypass flag. When `true`, `ring_push_blank`'s eviction
    /// step skips the SlimCell intern + `scrollback_slim` / `scrollback_wrapped`
    /// push/pop work (and the `release_slim_row` dec-ref loop). Instead it
    /// updates a virtual scrollback length so the externally observable
    /// bookkeeping — `get_scrollback_length()` and `scrollback_evicted_total`
    /// — is byte-identical to today's live path. Used only inside
    /// `TerminalCore::build_from_snapshot` to drop the per-row compression
    /// cost during a closed-form payload replay where the scrollback contents
    /// are not needed.
    pub(crate) scrollback_bypass: bool,
    /// Gates [`Self::push_pending_prompt_mark`]'s eager B-mark text capture
    /// into `bypass_b_mark_texts` (D4'''', round-7 rework, review round-6
    /// finding `0bed3c30e41e2389`).
    ///
    /// Deliberately SEPARATE from `scrollback_bypass`: that flag tracks
    /// whether the ring-eviction FAST PATH is active RIGHT NOW, but
    /// `build_from_snapshot_inner`'s D1''' prefix/suffix split replays its
    /// PREFIX with `scrollback_bypass` still `false` (a plain,
    /// full-fidelity replay) and only turns bypass on afterward for the
    /// suffix — yet the prefix's real scrollback is ALSO about to be
    /// discarded (folded into virtual bookkeeping by
    /// `restore_bypass_invariant_after_reflow`, right before bypass turns
    /// on). Gating capture on `scrollback_bypass` alone therefore missed
    /// every B mark emitted during the prefix: not captured into
    /// `bypass_b_mark_texts` (bypass was off when it fired), and not
    /// recoverable from scrollback either (the prefix's real rows are
    /// gone by the time the consumer looks). This flag instead spans the
    /// WHOLE bypass-engaged replay (prefix AND suffix) — set to
    /// `bypass_engaged` once, immediately after `reset()`, before any
    /// bytes are replayed — so a prefix-phase B mark is captured exactly
    /// like a suffix-phase one. `false` on the non-bypass whole-drain path
    /// (`build_scrollback_only_from_snapshot`) and on the live-PTY path,
    /// matching `bypass_b_mark_texts`'s existing "only populated during a
    /// snapshot-replay bypass" contract. Reset to `false` by
    /// [`Self::disable_snapshot_bypass`] and [`Self::reset`].
    pub(crate) capture_bypass_b_marks: bool,
    /// Stand-in for `scrollback_count() as u32` while
    /// `scrollback_bypass` is on. Reset to `0` when the bypass is enabled
    /// or disabled. On each `ring_push_blank` eviction under bypass: if
    /// `< scrollback_capacity`, increment; once equal to capacity, further
    /// scroll-offs bump `scrollback_evicted_total` instead. This makes the
    /// observable `get_scrollback_length()` value and the mark stamping
    /// site (`abs_row = get_scrollback_length() + cursor.row`) byte-identical
    /// to today's path on the same payload + same capacity.
    pub(crate) virtual_scrollback_len: u32,
    // Intern tables backing scrollback SlimCells.
    pub(crate) styles: StyleTable,
    pub(crate) chars: CharTable,
    pub(crate) dirty: Vec<u64>,
    pub(crate) cursor: CursorState,
    pub(crate) saved_cursor: Option<CursorState>,
    /// Settings-derived default cursor shape (0=block, 1=underline, 2=bar).
    /// Terminal-level state (cursor-settings-fix D1), outside `CursorState`,
    /// so DECSC/DECRC save/restore cannot alter it. Updated only by
    /// `set_cursor_style` (settings apply); `get_cursor_style()` returns
    /// `cursor_style_override` instead whenever one is active.
    pub(crate) cursor_style_default: u8,
    /// Settings-derived default cursor blink. Mirrors `cursor_style_default`
    /// for blink: terminal-level, updated only by `set_cursor_blink`.
    pub(crate) cursor_blink_default: bool,
    /// Active DECSCUSR / OSC 22 shape override, if any. `None` means
    /// `get_cursor_style()` falls back to `cursor_style_default`. Cleared by
    /// RIS and by DECSCUSR Ps=0/absent; set by DECSCUSR (shape+blink
    /// together) and by OSC 22 (shape only).
    pub(crate) cursor_style_override: Option<u8>,
    /// Active DECSCUSR blink override, if any. `None` means
    /// `get_cursor_blink()` falls back to `cursor_blink_default`. Cleared by
    /// RIS and by DECSCUSR Ps=0/absent; set only by DECSCUSR (OSC 22 never
    /// touches blink, per SPEC FR4).
    pub(crate) cursor_blink_override: Option<bool>,
    pub(crate) modes: u32,
    pub(crate) tab_stops: Vec<bool>,
    pub(crate) overflow: OverflowTable,
    pub(crate) overflow_ridx: OverflowRowIndex,
    // Sprint 2: Print handler state
    pub(crate) grapheme_buffer: Vec<u32>,
    pub(crate) wrap_pending: bool,
    /// Viewport position `(col, row)` of the most recently written grid
    /// cell in the print path — always the BASE cell of the last-written
    /// grapheme (never a wide-char spacer). This is the merge target for a
    /// standalone-arriving zero-width character (VARIATION_SEL / COMBINING,
    /// see `crate::print_handler::try_retroactive_merge`). `None` means
    /// there is no valid target: nothing has been written yet on this
    /// screen, or the tracked position was invalidated by cursor movement,
    /// scrolling, an erase, a resize, or a reset. Conservative invalidation
    /// is intentional: dropping a combining character is preferable to
    /// merging it into an unrelated cell.
    pub(crate) last_write: Option<(u16, u16)>,
    pub(crate) g0_charset: u8,     // 0=Ascii, 1=DecLineDrawing
    pub(crate) g1_charset: u8,     // 0=Ascii, 1=DecLineDrawing
    pub(crate) active_charset: u8, // 0=G0, 1=G1
    /// Suppress Kitty Unicode placeholder characters (U+10EEEE + combining marks).
    /// Set when U+10EEEE is received; cleared on next non-combining codepoint.
    pub(crate) kitty_placeholder_active: bool,
    pub(crate) scroll_region_top: u16,
    pub(crate) scroll_region_bottom: u16,
    // Sprint 4: Device response buffer
    pub(crate) response_buffer: [u8; 64],
    pub(crate) response_len: u8,
    // Cell size in pixels (for CSI 14t/16t responses)
    pub(crate) cell_width_px: u16,
    pub(crate) cell_height_px: u16,
    // Scroll event for differential rendering
    pub(crate) scroll_event: Option<crate::ring_buffer::ScrollEvent>,
    // Sprint 6: Parser and mode action queue
    pub(crate) parser: crate::parser::Parser,
    pub(crate) mode_actions: Vec<u8>,
    /// Side-effect sink for OSC / APC / DCS / BEL / device-response.
    /// `None` = silently drop (matches the previous wasm-no-callback behaviour).
    pub callbacks: Option<Box<dyn TerminalCallbacks>>,
    // Hyperlink table: maps hyperlink_id -> (params, uri)
    pub(crate) hyperlink_table: Vec<Option<(String, String)>>,
    pub(crate) hyperlink_next_id: u16,
    pub(crate) active_hyperlink_id: u16,
    /// Set when cursor transitions hidden→visible (DECTCEM set while previously hidden).
    /// Used by process_pty_data to interrupt parsing so the JS side can render
    /// the intermediate state (e.g., vim's search wrap message).
    pub(crate) cursor_just_shown: bool,
    /// When true, cursor hidden→visible transitions interrupt parsing.
    /// Disable if it causes flicker in applications that frequently toggle cursor.
    pub(crate) cursor_show_interrupt: bool,
    /// OSC 133 semantic-prompt marks accumulated *during* parsing, each
    /// stamped with the absolute row (`scrollback_len + cursor.row`) and the
    /// eviction counter at the moment the OSC handler ran. Native consumers
    /// (native-poc) drain this via `take_prompt_marks` after the pump so
    /// each mark keeps the row it was actually emitted on, instead of all
    /// marks in one chunk collapsing onto the final cursor row. The wasm /
    /// WebView path ignores this and keeps using the `on_osc(133, …)`
    /// callback, which still fires. Capped at `MAX_PENDING_PROMPT_MARKS`.
    pub(crate) pending_prompt_marks: VecDeque<PendingPromptMark>,
    /// Custom-fold marks (`OSC 777;emterm;fold;begin|end`) accumulated
    /// *during* parsing, each stamped with the absolute row it was emitted
    /// on (`scrollback_len + cursor.row`) and the eviction counter at that
    /// instant. Native consumers (native-poc) drain this via
    /// `take_fold_marks` after the pump and do the begin↔end pairing
    /// themselves. The wasm / WebView path ignores this entirely and keeps
    /// using the `on_osc(777, …)` callback, which still fires (so the
    /// status-bar dispatcher / legacy viewer queue are unaffected).
    /// Suppressed on the alternate screen, matching the OSC 133 capture and
    /// the WebView `isAlternateBuffer` guard. Capped at
    /// `MAX_PENDING_FOLD_MARKS`.
    pub(crate) pending_fold_marks: VecDeque<PendingFoldMark>,
    /// Side-table populated only during a snapshot replay whose PREFIX or
    /// SUFFIX (or both) will discard real scrollback content — gated by
    /// [`Self::capture_bypass_b_marks`], not directly by
    /// `scrollback_bypass` (D4'''', round-7 rework, see that field's doc
    /// for why). When an OSC 133 B (CommandStart) mark is emitted while
    /// capture is on, the plain text of the cursor row at that instant is
    /// captured here under `abs_row → text`. This is necessary because the
    /// bypass intentionally discards scrollback contents: once the row
    /// scrolls past the viewport into the virtual scrollback it is
    /// irrecoverable. The downstream consumer
    /// (`tabs.rs::extract_line_text`) prefers this pre-captured text over a
    /// scrollback lookup when processing the drained `SnapshotReplay`.
    /// Drained by `take_bypass_b_mark_texts` (called from
    /// `build_from_snapshot` and shipped on `SnapshotReplay`). Cleared by
    /// `reset()`. Only populated during a bypass-engaged snapshot replay;
    /// remains empty on the normal live-PTY path.
    pub(crate) bypass_b_mark_texts: std::collections::HashMap<u32, String>,
    /// Application-layer OSC parameter overrides: `(osc_param, action_type)`.
    ///
    /// `term_core` knows no application protocol numbers. A host that layers
    /// its own protocol on a private OSC parameter (e.g. the mux inband frame
    /// param) registers it here via [`Self::register_osc_app_param`]; an OSC
    /// whose param `term_core` does not natively handle is mapped to the
    /// registered `action_type` and delivered through `on_osc`, so the host's
    /// callback can recognize it. Empty by default (vanilla terminal core).
    pub(crate) osc_app_params: Vec<(u16, u8)>,
    /// Cumulative count of [`Self::resize`] calls (each one a full
    /// content-preserving reflow) since construction. A diagnostic-only
    /// counter — not touched by [`Self::reset`] — used to observe reflow
    /// cost from a test (before/after delta around a specific replay call).
    /// Introduced for review round-1 rework, finding `6ff208bbc674189c`
    /// (task0002 AC-5): proves a run of consecutive resize markers with no
    /// bytes between them costs at most ONE reflow, not one per marker.
    pub(crate) reflow_call_count: u64,
}

impl TerminalCore {
    pub fn new(cols: u16, rows: u16, scrollback_lines: u32) -> Self {
        debug_assert!(cols > 0 && rows > 0, "cols and rows must be > 0");
        let scrollback_capacity = scrollback_lines as usize;
        let ring_capacity = scrollback_capacity + rows as usize;
        // Viewport ring is sized for `rows` lines only; scrollback lives in
        // a separate compressed deque (scrollback_slim).
        let total = rows as usize * cols as usize;
        let dirty_words = (rows as usize + 63) / 64;

        // Default modes: autoWrap=true, cursorVisible=true, cursorBlink=true,
        // alternateScroll=true (DECSET 1007).
        let default_modes = (1u32 << MODE_AUTO_WRAP)
            | (1u32 << MODE_CURSOR_VISIBLE)
            | (1u32 << MODE_CURSOR_BLINK)
            | (1u32 << MODE_ALTERNATE_SCROLL);

        let mut tab_stops = vec![false; cols as usize];
        for i in (0..cols as usize).step_by(8) {
            tab_stops[i] = true;
        }

        let mut core = Self {
            cols,
            rows,
            ring_cells: vec![Cell::EMPTY; total],
            ring_wrapped: vec![false; rows as usize],
            ring_head: 0,
            ring_size: rows as usize,
            ring_capacity,
            scrollback_slim: VecDeque::with_capacity(scrollback_capacity.min(64)),
            scrollback_wrapped: VecDeque::with_capacity(scrollback_capacity.min(64)),
            scrollback_capacity,
            scrollback_evicted_total: 0,
            scrollback_bypass: false,
            capture_bypass_b_marks: false,
            virtual_scrollback_len: 0,
            styles: StyleTable::new(),
            chars: CharTable::new(),
            dirty: vec![u64::MAX; dirty_words], // all dirty initially
            cursor: CursorState::new(),
            saved_cursor: None,
            cursor_style_default: 0,
            cursor_blink_default: true,
            cursor_style_override: None,
            cursor_blink_override: None,
            modes: default_modes,
            tab_stops,
            overflow: OverflowTable::new(),
            overflow_ridx: OverflowRowIndex::new(),
            // Sprint 2
            grapheme_buffer: Vec::with_capacity(8),
            wrap_pending: false,
            last_write: None,
            g0_charset: 0,
            g1_charset: 0,
            active_charset: 0,
            kitty_placeholder_active: false,
            scroll_region_top: 0,
            scroll_region_bottom: rows.saturating_sub(1),
            // Sprint 4
            response_buffer: [0u8; 64],
            response_len: 0,
            cell_width_px: 8,
            cell_height_px: 16,
            // Scroll event
            scroll_event: None,
            // Sprint 6
            parser: crate::parser::Parser::new(),
            mode_actions: Vec::new(),
            // Sprint 6: Callbacks
            callbacks: None,
            // Hyperlink
            hyperlink_table: vec![None], // index 0 = no hyperlink
            hyperlink_next_id: 1,
            active_hyperlink_id: 0,
            cursor_just_shown: false,
            cursor_show_interrupt: false,
            pending_prompt_marks: VecDeque::new(),
            pending_fold_marks: VecDeque::new(),
            bypass_b_mark_texts: std::collections::HashMap::new(),
            osc_app_params: Vec::new(),
            reflow_call_count: 0,
        };
        core.mark_all_dirty();
        core
    }

    /// Register an application-layer OSC parameter → `action_type` mapping.
    ///
    /// `term_core` itself embeds no application protocol numbers. The host
    /// calls this for each private OSC parameter it owns (e.g. the mux inband
    /// frame param); a subsequent OSC carrying that param — and not natively
    /// handled by `term_core` — is delivered via `on_osc(action_type, data)`
    /// so the host's callback can recognize and route it. The host is
    /// responsible for choosing an `action_type` that does not collide with
    /// `term_core`'s native OSC action types.
    pub fn register_osc_app_param(&mut self, param: u16, action_type: u8) {
        self.osc_app_params.push((param, action_type));
    }

    // ── Grid dimensions ──────────────────────────────────

    pub fn cols(&self) -> u16 {
        self.cols
    }

    pub fn rows(&self) -> u16 {
        self.rows
    }

    /// Maximum number of scrollback lines this core retains (the
    /// `scrollback_lines` it was constructed with). Used by the mux
    /// off-thread snapshot replay to size the worker-built core to the same
    /// scrollback depth as the live core it will replace.
    pub fn scrollback_capacity(&self) -> u32 {
        self.scrollback_capacity as u32
    }

    /// Cumulative count of [`Self::resize`] calls (each one a full
    /// content-preserving reflow) since construction — see the
    /// `reflow_call_count` field doc for the full rationale. `pub` so
    /// external crates (the `emterm` daemon's regression tests) can observe
    /// it directly: a replay that recognizes and applies NO resize marker
    /// at all — including one that is present in the byte stream but
    /// correctly treated as forged / inert — never calls [`Self::resize`],
    /// so this stays at its pre-replay value. This is the reliable way to
    /// prove "no marker was honored" for a forged-marker regression test:
    /// checking the core's FINAL `cols()`/`rows()` after a full
    /// `reset_and_replay` does NOT work for this, because
    /// `replay_with_resize_markers` unconditionally restores the core to
    /// its construction/target dimensions at the end of every replay
    /// regardless of what happened (or didn't) mid-stream — a genuinely
    /// honored forged marker and a correctly-ignored one both end up back
    /// at the same final size.
    pub fn reflow_call_count(&self) -> u64 {
        self.reflow_call_count
    }

    /// Set cell size in pixels (for CSI 14t/16t XTWINOPS responses).
    /// Called from TypeScript after measuring character dimensions.
    pub fn set_cell_size_px(&mut self, width: u16, height: u16) {
        self.cell_width_px = width;
        self.cell_height_px = height;
    }

    /// Get cell width in pixels.
    pub fn get_cell_width_px(&self) -> u16 {
        self.cell_width_px
    }

    /// Get cell height in pixels.
    pub fn get_cell_height_px(&self) -> u16 {
        self.cell_height_px
    }

    // ── Resize ───────────────────────────────────────────

    /// Legacy resize (delegates to resize_reflow with scrollback_lines=0).
    /// Kept for backward compatibility with existing tests.
    pub fn resize(&mut self, new_cols: u16, new_rows: u16) {
        // A resize/reflow can relocate or drop any cell; the most-recently-
        // written-cell tracking used by retroactive zero-width merge can no
        // longer be trusted afterwards.
        self.last_write = None;
        self.reflow_call_count += 1;
        // Use reflow with current scrollback capacity
        let scrollback_lines = self.ring_capacity.saturating_sub(self.rows as usize) as u32;
        self.resize_reflow(new_cols, new_rows, scrollback_lines);
        // Bypass invariant (review round-1 rework, finding
        // `1698d9b52a89e241`, medium but correctness-relevant): the
        // content-preserving `resize_reflow` above is NOT bypass-aware —
        // called mid-drain while `scrollback_bypass` is on (a resize marker
        // inside `replay_with_resize_markers` during the off-thread 1st-pass
        // snapshot replay, `build_from_snapshot`), it can populate
        // `scrollback_slim` with real rows even though the bypass's whole
        // point is to keep that deque empty. If left in place, the 2nd-pass
        // `merge_scrollback_from` later mistakes these leaked rows for
        // genuine post-swap live-drain content (they sit at the FRONT of
        // `self.scrollback_slim` exactly where merged rows get prepended),
        // causing row duplication / reordering — not merely an accounting
        // miscount. Restore the invariant on every call while bypass is
        // active so it never has a chance to leak past this function.
        if self.scrollback_bypass {
            self.restore_bypass_invariant_after_reflow();
        }
    }

    /// Drain whatever `scrollback_slim` currently holds, folding its length
    /// into the SAME virtual bookkeeping (`virtual_scrollback_len` /
    /// `scrollback_evicted_total`) [`Self::enable_snapshot_bypass`]
    /// documents — i.e. treat those rows exactly as `ring_push_blank`'s
    /// bypass branch would have, had it been the one to evict them, so
    /// bookkeeping stays byte-identical instead of merely "close".
    ///
    /// Two call sites:
    /// - [`Self::resize`], while `scrollback_bypass` is already on: cleans
    ///   up rows a mid-drain content-preserving reflow leaked into
    ///   `scrollback_slim` despite the bypass (see that method's call
    ///   site).
    /// - `build_from_snapshot_inner`'s D1''' (round-6 rework) prefix/suffix
    ///   split, BEFORE `scrollback_bypass` is turned on: converts a
    ///   non-bypass prefix's REAL scrollback into the bypass's virtual
    ///   count, so `enable_snapshot_bypass`'s "empty deque" precondition
    ///   holds for the suffix even though real content already scrolled
    ///   during the prefix.
    fn restore_bypass_invariant_after_reflow(&mut self) {
        let leaked = self.scrollback_slim.len();
        if leaked == 0 {
            debug_assert!(
                self.scrollback_wrapped.is_empty(),
                "scrollback_wrapped must track scrollback_slim 1:1"
            );
            return;
        }
        let drained: Vec<Vec<SlimCell>> = self.scrollback_slim.drain(..).collect();
        for row in &drained {
            self.release_slim_row(row);
        }
        self.scrollback_wrapped.clear();
        let capacity = self.scrollback_capacity as u64;
        let total = self.virtual_scrollback_len as u64 + leaked as u64;
        if total <= capacity {
            self.virtual_scrollback_len = total as u32;
        } else {
            self.virtual_scrollback_len = capacity as u32;
            self.scrollback_evicted_total += total - capacity;
        }
    }

    // ── Scroll Event ─────────────────────────────────────

    /// Returns the scroll event direction: 1=Up, 0=none.
    pub fn get_scroll_event_direction(&self) -> u8 {
        match &self.scroll_event {
            Some(e) => match e.direction {
                crate::ring_buffer::ScrollDirection::Up => 1,
            },
            None => 0,
        }
    }

    /// Returns the scroll event count (0 if no event).
    pub fn get_scroll_event_count(&self) -> u16 {
        self.scroll_event.as_ref().map_or(0, |e| e.count)
    }

    /// Clears the pending scroll event.
    pub fn clear_scroll_event(&mut self) {
        self.scroll_event = None;
    }

    // ── Reset ────────────────────────────────────────────

    /// Feed `bytes` until every byte is consumed, resuming across the
    /// parser's deliberate interrupts, and return the accumulated mode
    /// actions.
    ///
    /// `process_pty_data` returns early (with the consumed byte count) on a
    /// pending buffer switch (CSI ?47/?1047/?1049) or a hidden→visible
    /// cursor transition so the embedder can react mid-stream — the WebView
    /// build pauses to let JS swap buffers before re-invoking. An embedder
    /// with no mid-stream reaction (native-poc) that calls
    /// `process_pty_data` once and ignores the return value silently DROPS
    /// the remainder of the chunk (e.g. everything after `?1049l` in the
    /// same PTY read as a vim exit). This resume loop drains
    /// `take_mode_actions` between rounds (a pending buffer switch would
    /// otherwise re-interrupt the very next call) and hands the drained
    /// actions back for the caller's alt-screen tracking.
    pub fn process_pty_data_fully(&mut self, bytes: &[u8]) -> Vec<u8> {
        // The non-cancellable entry point (22+ call sites). It delegates to
        // the cancellable drain with a flag that is never set, so there is a
        // single resume-loop implementation and the two paths cannot drift.
        // `NEVER` is never stored to, so the cancellable drain always runs to
        // completion and returns `Some` — the unwrap cannot fail.
        static NEVER: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
        self.process_pty_data_fully_cancellable(bytes, &NEVER)
            .expect("non-cancellable drain always completes")
    }

    /// Cancellable variant of [`Self::process_pty_data_fully`]. Checks
    /// `cancel` at each resume-loop boundary and returns `None` if it is set
    /// mid-drain, so an off-thread snapshot worker whose switch was superseded
    /// can bail out at the next chunk boundary instead of parsing the whole
    /// payload (bounding wasted work + concurrent worker lifetime under a
    /// rapid pane-switch / resize storm). Returns `Some(actions)` on a
    /// completed drain, identical to the non-cancellable path.
    pub fn process_pty_data_fully_cancellable(
        &mut self,
        bytes: &[u8],
        cancel: &std::sync::atomic::AtomicBool,
    ) -> Option<Vec<u8>> {
        let mut actions: Vec<u8> = Vec::new();
        let mut offset = 0usize;
        while offset < bytes.len() {
            // Cooperative cancellation: a relaxed load per chunk is negligible
            // on the live-output hot path; a superseded worker stops here.
            if cancel.load(std::sync::atomic::Ordering::Relaxed) {
                return None;
            }
            let consumed = self.process_pty_data(&bytes[offset..]);
            offset += consumed;
            actions.extend(self.take_mode_actions());
            if consumed == 0 {
                // Defensive: the parser should always make progress, but
                // never spin if it reports zero consumption.
                break;
            }
        }
        Some(actions)
    }

    /// Reset the grid + parser to the post-construction state, then replay
    /// `bytes` so the resulting state reflects a fresh replay of that byte
    /// stream. Returns the mode actions accumulated during the replay (a
    /// snapshot captured while a full-screen app was running carries its
    /// buffer-switch sequences).
    ///
    /// Introduced for native-poc's mux-mode attach: after the daemon sends
    /// a `Snapshot`, the client wants to discard whatever the native PTY
    /// painted previously and paint the snapshot bytes from scratch. Uses
    /// the resume loop (`process_pty_data_fully`) — a single
    /// `process_pty_data` call would drop everything after the first
    /// buffer-switch sequence inside the snapshot.
    ///
    /// Equivalent to [`Self::reset_and_replay_segments`] with an empty
    /// segment list — a single, unsplit replay at `self`'s current
    /// dimensions (task0004 round-4 rework D1' / AC-11: the documented
    /// "no structural dimension info" degradation — this is what an older
    /// daemon's snapshot, or any caller with nothing to attribute, gets).
    pub fn reset_and_replay(&mut self, bytes: &[u8]) -> Vec<u8> {
        self.reset_and_replay_segments(bytes, &[])
    }

    /// Reset the grid + parser to the post-construction state, then replay
    /// `bytes` under the dimensions `segments` describes structurally.
    ///
    /// This is the D1' replacement for the in-band `OSC 777;emterm;resize;…`
    /// marker byte scan (rounds 1-3, `mux::scrollback_buffer`
    /// `resize_marker_bytes` / `find_resize_marker`): dimensions are supplied
    /// HERE, as a caller-provided parameter, never discovered by scanning
    /// `bytes` for a recognizable pattern. No byte sequence appearing
    /// anywhere in `bytes` — however it is shaped, split, or nested — can
    /// therefore ever change what dimensions a replay applies; the forgery
    /// class rounds 1-3 spent three attempts trying to filter out of the
    /// byte stream is closed structurally instead (there is nothing left
    /// that scans for one).
    ///
    /// `segments` must be in ascending `offset` order (the caller's
    /// responsibility — mirrors the ordering invariant the daemon-side
    /// `dim_markers` structure already keeps). For each segment, in order,
    /// `self` is resized to `(segment.cols, segment.rows)` (only when they
    /// differ from the current size) and then fed the byte range from this
    /// segment's `offset` up to the NEXT segment's `offset` (or the end of
    /// `bytes` for the last segment). An `offset` past `bytes.len()` is
    /// clamped. After the last segment (or immediately, if `segments` is
    /// empty), `self` is resized back to its dimensions at the START of this
    /// call (the caller's requested / current pane size) if anything
    /// changed them, so a replay with any number of intervening resizes
    /// always ends at the size the caller asked for — matching the old
    /// marker-scan replay's contract exactly, just driven by `segments`
    /// instead of a byte scan.
    ///
    /// An empty `segments` reduces to a single unsplit
    /// `process_pty_data_fully_cancellable` call at `self`'s current
    /// dimensions — byte-for-byte identical to the pre-task0001 replay
    /// (task0001 AC-3 / task0004 AC-11).
    pub fn reset_and_replay_segments(
        &mut self,
        bytes: &[u8],
        segments: &[ReplaySegment],
    ) -> Vec<u8> {
        self.reset();
        // The non-cancellable entry point: delegates to the cancellable
        // drain with a flag that is never set, so `reset_and_replay_segments`
        // and `build_from_snapshot`'s cancellable drain share one replay
        // implementation and cannot drift. `NEVER` is never stored to, so
        // the drain always runs to completion and returns `Some` — the
        // unwrap cannot fail.
        static NEVER: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
        let (final_cols, final_rows) = (self.cols, self.rows);
        self.replay_segments(bytes, segments, &NEVER, final_cols, final_rows)
            .expect("non-cancellable drain always completes")
    }

    /// Cancellable implementation shared by [`Self::reset_and_replay_segments`]
    /// and `build_from_snapshot_inner`. See
    /// [`Self::reset_and_replay_segments`] for the segment-driven replay
    /// contract; `cancel` is threaded straight through to each segment's
    /// `process_pty_data_fully_cancellable`, and a flag observed mid-drain
    /// aborts the whole replay and returns `None` (a superseded off-thread
    /// `build_from_snapshot` worker bails out at the next chunk boundary
    /// instead of finishing the parse).
    ///
    /// `final_cols`/`final_rows` is the size `self` is resized back to (if
    /// anything changed it) once every segment has replayed — the caller's
    /// requested / current pane size. task0004 (D8, review round-1 rework,
    /// finding `b21749c5f2bd1006`) made this an explicit parameter rather
    /// than `self.cols`/`self.rows` captured at entry: `build_from_snapshot_inner`'s
    /// MIDDLE sub-replay now calls this with `self` starting at the HEAD's
    /// own (possibly non-target) dimensions, but must still end at the
    /// TRUE caller target in one hop — capturing `self.cols`/`self.rows` at
    /// entry would resize back to the HEAD's dimensions instead, an extra
    /// (and potentially non-equivalent) resize hop the reference path never
    /// takes. `reset_and_replay_segments` passes its own `self.cols`/
    /// `self.rows` at entry, preserving this method's original behavior for
    /// every other caller.
    fn replay_segments(
        &mut self,
        bytes: &[u8],
        segments: &[ReplaySegment],
        cancel: &std::sync::atomic::AtomicBool,
        final_cols: u16,
        final_rows: u16,
    ) -> Option<Vec<u8>> {
        let target_cols = final_cols;
        let target_rows = final_rows;
        let mut actions = Vec::new();
        if segments.is_empty() {
            actions.extend(self.process_pty_data_fully_cancellable(bytes, cancel)?);
            return Some(actions);
        }
        // D1''''' (round-8 rework, review round-7 finding `01f91fe698ceb287`):
        // `segments` is no longer guaranteed to have its first entry at
        // offset 0 — the daemon-side cap-eviction gap (2+ evicted
        // `dim_markers` entries) is now left unattributed by
        // `ScrollbackRingBuffer::read_segments` rather than synthesizing a
        // (potentially wrong) head segment for it. Replay that leading gap,
        // if any, BEFORE the first segment's own dims are applied — at
        // whatever dims `self` already has, which is exactly
        // `target_cols`/`target_rows` here since nothing has resized `self`
        // yet. This is what "leave the gap unattributed" cashes out to at
        // replay time: those bytes replay under the caller's TARGET size,
        // never silently dropped.
        let first_offset = (segments[0].offset as usize).min(bytes.len());
        if first_offset > 0 {
            actions
                .extend(self.process_pty_data_fully_cancellable(&bytes[..first_offset], cancel)?);
        }
        for (i, seg) in segments.iter().enumerate() {
            let start = (seg.offset as usize).min(bytes.len());
            let end = segments
                .get(i + 1)
                .map(|next| (next.offset as usize).min(bytes.len()))
                .unwrap_or(bytes.len());
            // The resize is applied ONLY when this segment actually has
            // content to feed (`end > start`) — mirroring the round-1
            // rework reflow-coalescing fix (finding `6ff208bbc674189c`): a
            // run of segments whose content ranges are all empty (their
            // offsets collapse together — no real bytes were ever recorded
            // at those intermediate dimensions) costs ZERO reflows for the
            // empty ones. Only the segment that actually has bytes to feed
            // pays a reflow, for its OWN dimensions — never one reflow per
            // segment regardless of content.
            if end > start {
                // D1'' (task0005 rework, review round-4 finding
                // `da834d05f3f18af4`, high): a decoded segment travels
                // straight from the wire (`mux_ipc::protocol::DimSegment`,
                // an untrusted `u16` pair) to here. Without this clamp,
                // `self.resize` allocates `(scrollback_capacity + rows) *
                // cols` cells unconditionally — a segment carrying
                // `cols == rows == 65535` requests roughly 4.3 billion
                // cells, and a zero dimension trips `resize_reflow`'s
                // `debug_assert!(cols > 0 && rows > 0)` (an underflow in
                // release builds). `clamp_resize_dims` is the SAME domain
                // the daemon-side producer already enforces
                // (`MuxPane::resize` / `MuxPane::new`) — applying it again
                // here means a segment can never resize this core outside
                // that domain regardless of what produced it (a forged
                // frame, a future encoder bug, or a daemon that predates
                // the producer-side clamp).
                let (seg_cols, seg_rows) = clamp_resize_dims(seg.cols, seg.rows);
                if (self.cols, self.rows) != (seg_cols, seg_rows) {
                    self.resize(seg_cols, seg_rows);
                }
                actions
                    .extend(self.process_pty_data_fully_cancellable(&bytes[start..end], cancel)?);
            }
        }
        if (self.cols, self.rows) != (target_cols, target_rows) {
            self.resize(target_cols, target_rows);
        }
        Some(actions)
    }

    /// Construct a fresh `TerminalCore` sized to `(cols, rows,
    /// scrollback_lines)`, full-drain replay `payload` into it, and return
    /// the built core together with the mode actions and the
    /// prompt / fold marks (plus the post-replay eviction total) drained
    /// during the replay.
    ///
    /// This is the **pure, off-thread half** of the mux snapshot-replay
    /// recipe: it owns and returns the core (no `&mut self`), installs no
    /// callbacks, and touches no GUI / thread-local state, so it can run on
    /// a worker thread and the result moved to the main thread. The
    /// returned bundle is observably identical to the in-place
    /// `reset_and_replay` + `drain` path on a core of the same size fed the
    /// same `payload` for the externally observable bookkeeping — the
    /// `evicted_total`, `prompt_marks` (`abs_row` + `evicted_total`), and
    /// `fold_marks` (`abs_row` + `evicted_total`) match byte-identically —
    /// and the viewport grid + cursor are byte-identical. The off-thread
    /// path and the synchronous path therefore reconcile from
    /// byte/grid-identical inputs.
    ///
    /// **Scrollback contents are intentionally not populated by the replay.**
    /// During the drain the per-row SlimCell compression (the dominant cost
    /// on a heavy `seq`-shaped payload) is bypassed; `core.scrollback_count()`
    /// is `0` on the returned core. The `scrollback_capacity` is the
    /// caller-requested `scrollback_lines`, so any live PTY appends to the
    /// returned core accumulate into scrollback exactly as they do today.
    /// The bypass keeps the observable bookkeeping byte-identical via an
    /// internal virtual scrollback length (see
    /// `TerminalCore::scrollback_bypass` / `virtual_scrollback_len`).
    ///
    /// A fresh `TerminalCore::new` is already in the post-`reset` state, so
    /// the extra `reset` inside `reset_and_replay` is a no-op here; it is
    /// kept so the off-thread builder and the synchronous path share the
    /// exact same replay entry point (`reset_and_replay`) and cannot drift.
    ///
    /// The drained marks/total are returned (rather than left on the core)
    /// because the caller backfills them into its own absolute-row trackers
    /// after the swap, exactly as the synchronous `drain_marks` site does.
    /// `cancel` lets a superseded off-thread worker abandon the parse at the
    /// next chunk boundary; when it is observed set mid-drain this returns
    /// `None` (the partially-built core is discarded by the caller).
    pub fn build_from_snapshot(
        cols: u16,
        rows: u16,
        scrollback_lines: u32,
        payload: &[u8],
        segments: &[ReplaySegment],
        cancel: &std::sync::atomic::AtomicBool,
    ) -> Option<SnapshotReplay> {
        Self::build_from_snapshot_inner(
            cols,
            rows,
            scrollback_lines,
            payload,
            segments,
            cancel,
            true,
        )
    }

    /// Sibling of [`Self::build_from_snapshot`] that runs the same replay
    /// **with the snapshot bypass disabled**. The drained core therefore has
    /// its `scrollback_slim` / `scrollback_wrapped` populated up to
    /// `scrollback_lines` rows, which is what the 2nd-pass scrollback-restore
    /// worker needs to feed [`Self::merge_scrollback_from`].
    ///
    /// Observable bookkeeping matches the synchronous `reset_and_replay`
    /// path byte-identically (same `evicted_total`, same prompt/fold marks,
    /// same grid). `bypass_b_mark_texts` is empty because the bypass is off
    /// and the live scrollback is the source of truth for B-mark texts —
    /// the caller MUST ignore that field on the result (FR8).
    ///
    /// `cancel` semantics are identical to `build_from_snapshot`: a set flag
    /// observed mid-drain returns `None` and the partially-built core is
    /// discarded.
    pub fn build_scrollback_only_from_snapshot(
        cols: u16,
        rows: u16,
        scrollback_lines: u32,
        payload: &[u8],
        segments: &[ReplaySegment],
        cancel: &std::sync::atomic::AtomicBool,
    ) -> Option<SnapshotReplay> {
        Self::build_from_snapshot_inner(
            cols,
            rows,
            scrollback_lines,
            payload,
            segments,
            cancel,
            false,
        )
    }

    /// Shared inner helper for [`Self::build_from_snapshot`] (bypass on) and
    /// [`Self::build_scrollback_only_from_snapshot`] (bypass off). The two
    /// sibling entry points are thin wrappers that only differ in whether
    /// `enable_snapshot_bypass` is called, which keeps the recipe (reset →
    /// drain → take marks → assemble `SnapshotReplay`) in one place.
    fn build_from_snapshot_inner(
        cols: u16,
        rows: u16,
        scrollback_lines: u32,
        payload: &[u8],
        segments: &[ReplaySegment],
        cancel: &std::sync::atomic::AtomicBool,
        bypass: bool,
    ) -> Option<SnapshotReplay> {
        // D1''' (round-6 rework, review round-5 findings `abb36fa1ad4c89ea`
        // / `986a3881b2b97a16`): rounds 1-5 downgraded to the non-bypass
        // recipe for the WHOLE drain the moment ANY segment differed from
        // the target — correct, but it turns an ORDINARY switch into the
        // full non-bypass cost, because the daemon's spawn-size head
        // marker (`MuxPane::new`'s hardcoded 80x24) differs from the GUI's
        // actual grid until the ring evicts it (~2 MiB later). Measured:
        // 7ms segment-free vs 170-220ms for a single differing segment,
        // even though the divergence risk (see D6's history below) only
        // ever concerns the RESIZE moments themselves, not the (typically
        // much larger) bytes that follow the LAST one.
        //
        // Fix: split the replay at the start of the trailing run of
        // segments that ALREADY carry `(cols, rows)` — the caller's target
        // — via `stable_target_suffix_start`. Everything before that point
        // (the "prefix") replays WITHOUT bypass: full content-preserving
        // resizes, correct, priced only by the prefix's OWN content
        // (typically tiny — an ordinary switch's prefix is just the bytes
        // between the daemon spawning the pane and the GUI's first resize).
        // Once the core reaches the target dimensions, NOTHING in the
        // remaining segments changes them again BY CONSTRUCTION of the
        // split point, so the suffix — the pane's actual history, the part
        // that dominates payload size — replays under the fast bypass path
        // with zero resize risk. `bypass_split` below is `None` when
        // there's no prefix to speak of (`k == 0`, the pre-round-6 fast
        // path — segments already open at the target) or the suffix is too
        // small to bother (`BYPASS_SUFFIX_MIN_BYTES`), in which case this
        // falls back to the SAME whole-drain non-bypass recipe rounds 1-5
        // used whenever `k > 0` (still correct — see D6 below for why it
        // must be correct, not merely fast).
        //
        // Viewport / cursor equivalence: `ring_push_blank`'s bypass branch
        // differs from the non-bypass branch ONLY in whether the evicted
        // row's content is compressed into real scrollback or counted
        // virtually — both branches advance `ring_head` / clear the new
        // viewport bottom identically either way. Since the suffix (by
        // construction) contains no resize, the viewport + cursor it
        // produces are therefore byte-identical whether or not bypass is
        // engaged for it; only the SCROLLBACK CONTENT differs (virtual vs
        // real), which is exactly what `scrollback_populated` already
        // exists to flag to the caller (the 2nd-pass background worker,
        // `tabs.rs::apply_offthread_swap`, fills it in for real). AC-5
        // equivalence for the ACTUAL split is
        // `bypass_split_matches_reference_viewport_and_cursor_for_ordinary_switch`
        // (below this fn's tests); the pre-existing `..._row_growing_marker`
        // / `..._cols_only_marker` fixtures cover the "no benefit" (`k ==
        // segments.len()`, empty suffix) case unchanged.
        //
        // D6 (task0003, review round-2 finding `893241823258fce3`) / D5''
        // (task0005, review round-4 finding `697d8dc2b88dcddc`): the reason
        // a resize genuinely needs the non-bypass recipe at all — a
        // row-count-GROWING (or column-changing) mid-drain resize needs to
        // pull rows up from / re-wrap real scrollback, and the bypass keeps
        // `scrollback_slim` deliberately empty, so doing that resize WHILE
        // bypassed diverges from the synchronous path. This still applies
        // to every resize up to and including the one that reaches the
        // target — `stable_target_suffix_start` never lets the split
        // engage bypass any earlier than that.
        let k = if bypass {
            stable_target_suffix_start(cols, rows, segments)
        } else {
            0
        };
        let split_at = segments
            .get(k)
            .map(|s| (s.offset as usize).min(payload.len()))
            .unwrap_or(payload.len());
        // D5'''' (round-7 rework, review round-6 finding `e519916efd5fdc42`):
        // also gate on the PREFIX being cheap (`split_at` is exactly its
        // byte length) — see `BYPASS_PREFIX_MAX_BYTES`'s doc for why an
        // expensive prefix makes the split pay its own "fast path" cost
        // twice instead of once.
        //
        // D5''''' (round-8 rework, review round-7 finding
        // `a4f4e36fef377d05`): the byte-only gate above still let a payload
        // that is OVERWHELMINGLY prefix engage the split — a 64 KiB prefix
        // (right at `BYPASS_PREFIX_MAX_BYTES`, inclusive) paired with just
        // over `BYPASS_SUFFIX_MIN_BYTES` (4096) of suffix is ~94% prefix by
        // volume, yet both individual thresholds are satisfied. Additionally
        // require the SUFFIX to actually DOMINATE the prefix
        // (`suffix_len >= split_at`) — not merely clear an absolute floor —
        // and bound the PREFIX's own segment count
        // (`k <= BYPASS_PREFIX_MAX_SEGMENTS`), independent of its byte
        // length: a prefix built from many small segments still pays one
        // reflow per segment regardless of how few total bytes they cover.
        let suffix_len = payload.len() - split_at;
        // D7 (task0001, NFR1-safe rescue for a resize-marker-dense tail):
        // `k`/`split_at` above find where the split's SUFFIX may safely
        // start, but the region BEFORE that point ("prefix", historically
        // treated as one expensive, non-bypass whole) can itself contain a
        // large LEADING run of segments that are already uniform in size —
        // swept in only because SOME later segment (still before the
        // stable tail) diverges from it. A pane whose recorded scrollback
        // has a dense cluster of resize markers near its tail (dims
        // wobbling away from and back to a settled size, e.g. during
        // status-bar settling) produces exactly this shape: `k` and
        // `split_at` land far past the cluster's own small footprint
        // because they are computed from the LAST divergence, dragging a
        // huge, already-safe HEAD along with the genuinely resize-needing
        // MIDDLE. `h` finds that leading safe run so the two can be told
        // apart; seeing `h` in the byte length is why `middle_len <
        // split_at` in that shape even though `middle_segment_count` stays
        // close to `k`.
        //
        // D8 (task0004, review round-1 rework, finding `b21749c5f2bd1006`):
        // the HEAD's own leading run need not be AT THE CALLER'S TARGET
        // dims — `leading_uniform_run_len` admits any uniform (target_cols,
        // R) run, reporting the run's own row count `R` alongside its
        // length. This is what makes a marker cluster that oscillates
        // ABOVE the settled target (the SPEC's actual measured direction —
        // `visible_row_count` 0→1 SHRINKS the grid, so the pre-settling
        // HEAD sits at the LARGER size) foldable: `R` becomes the safety
        // ceiling `middle_is_row_bounded` checks against below, in place of
        // `target_rows`. When the HEAD genuinely opens at the target
        // (`R == rows`, every pre-D8 shape), this is byte-identical to the
        // original `leading_target_run_len`.
        //
        // `R >= rows` is required IN ADDITION to `middle_is_row_bounded`:
        // once MIDDLE finishes, `replay_segments` resizes straight back to
        // the caller's `rows` in one hop (see that method's doc) — a
        // transition `middle_is_row_bounded` never itself examines, since
        // it is implicit, not one of `segments[h..k]`'s own entries. That
        // final hop is only ever a `<=R` move (safe, by the same argument)
        // when `rows <= R`; without this check a single ordinary-sized
        // leading segment (e.g. the daemon's spawn-size marker, which
        // trivially satisfies "a uniform run of length 1") would be folded
        // as a HEAD whose `R` is BELOW the target, silently discarding real
        // content the final grow-to-target then has no way to recover.
        //
        // D9 (task0004, review round-1 rework, finding `6a02ed7e1b606588`):
        // if the resulting HEAD cannot be safely folded (a column change, a
        // row count in MIDDLE exceeding the HEAD's own `R`, or `R < rows`),
        // degrade `h` all the way to `0` — the pre-D7 computation — rather
        // than gating `bypass_split` on a separate `head_fold_safe` flag.
        // Only ABANDONING the fold (not the whole split) means a shape that
        // engaged the split before D7 (e.g. a small target HEAD, a small
        // column-change MIDDLE, and a large target TAIL) still engages it
        // here: with `h == 0`, `middle_len == split_at` and
        // `middle_segment_count == k`, matching the pre-D7 gates exactly.
        let (h, head_rows) = if bypass && k > 0 {
            let (candidate_h, candidate_rows) = leading_uniform_run_len(cols, &segments[..k]);
            let candidate_safe = candidate_h > 0
                // `h == k` would leave an EMPTY MIDDLE, and `replay_segments`
                // early-returns for empty `segments` WITHOUT its "resize back
                // to the caller's target" step — the core would stay at
                // `head_rows` forever. Only fold when a real MIDDLE remains
                // to carry that final hop.
                && candidate_h < k
                && candidate_rows >= rows
                && middle_is_row_bounded(cols, candidate_rows, &segments[candidate_h..k]);
            if candidate_safe {
                (candidate_h, candidate_rows)
            } else {
                (0, rows)
            }
        } else {
            (0, rows)
        };
        let head_len = if h > 0 {
            segments
                .get(h)
                .map(|s| (s.offset as usize).min(payload.len()))
                .unwrap_or(payload.len())
        } else {
            0
        };
        let middle_len = split_at - head_len;
        let middle_segment_count = k - h;
        let bypass_split = bypass
            && k > 0
            && middle_segment_count <= BYPASS_PREFIX_MAX_SEGMENTS
            && suffix_len >= BYPASS_SUFFIX_MIN_BYTES
            && middle_len <= BYPASS_PREFIX_MAX_BYTES
            && suffix_len >= middle_len;
        // Whether `enable_snapshot_bypass` will actually be called anywhere
        // below — `k == 0` is the pre-round-6 "no transition at all" fast
        // path (segments already open at the target); `bypass_split` is the
        // D1''' prefix/suffix split. Both leave the core bypass-engaged by
        // the time the trailing bookkeeping below runs. Distinct from the
        // raw `bypass` parameter: a caller that requested bypass but whose
        // segments neither open at the target nor clear the split
        // threshold (`k > 0`, small tail) still gets the correct, but
        // non-bypassed, whole-drain replay — mirroring rounds 1-5's
        // downgrade for that shape exactly.
        let bypass_engaged = bypass_split || (bypass && k == 0);

        let mut core = TerminalCore::new(cols, rows, scrollback_lines);
        core.reset();
        // D4'''' (round-7 rework, review round-6 finding `0bed3c30e41e2389`):
        // set BEFORE any bytes are replayed (including the PREFIX, which
        // runs before `enable_snapshot_bypass` below) so a B mark emitted
        // during the prefix is captured just like one emitted during the
        // suffix — see `capture_bypass_b_marks`'s doc.
        core.capture_bypass_b_marks = bypass_engaged;

        let actions = if bypass_split {
            let (head_bytes, rest_bytes) = payload.split_at(head_len);
            let (middle_bytes, suffix_bytes) = rest_bytes.split_at(middle_len);
            // D7: `middle_segments` are `segments[h..k]` rebased so their
            // `offset`s are relative to `middle_bytes` (they were absolute
            // into `payload`, which starts `head_len` bytes earlier).
            let middle_segments: Vec<ReplaySegment> = segments[h..k]
                .iter()
                .map(|s| ReplaySegment {
                    offset: s.offset.saturating_sub(head_len as u32),
                    cols: s.cols,
                    rows: s.rows,
                })
                .collect();

            let mut actions = Vec::new();
            if head_len > 0 {
                // D8: the HEAD may open at `head_rows` rather than the
                // caller's `rows` (see the `h`/`head_rows` computation
                // above). `core` was just constructed + reset — completely
                // empty, no bytes replayed yet — so this resize is the
                // SAME operation the reference path performs for its own
                // first segment on an equally fresh core (see
                // `replay_segments`'s leading-gap handling): it cannot
                // diverge from the reference regardless of grow/shrink
                // direction, because there is no real content on either
                // side to lose. A shrink here deposits blank rows into
                // `scrollback_slim` for real (`resize`'s reflow is not
                // bypass-aware); fold them into the SAME virtual
                // bookkeeping the bypass path uses so `enable_snapshot_bypass`'s
                // "empty deque" precondition holds regardless of direction
                // (a no-op when the resize was a grow, which never adds to
                // `scrollback_slim`).
                if head_rows != rows {
                    core.resize(cols, head_rows);
                    core.restore_bypass_invariant_after_reflow();
                }
                // HEAD: every segment in `segments[..h]` already carries
                // `(cols, head_rows)` by construction of `h`, so — exactly
                // like the SUFFIX below — no further resize can occur
                // here; feed the bytes directly under bypass (cheap: no
                // SlimCell compression for content that was never going to
                // move dimensions). `scrollback_slim` is empty on entry
                // (either untouched, or just folded above), satisfying
                // `enable_snapshot_bypass`'s precondition.
                core.enable_snapshot_bypass();
                actions.extend(
                    match core.process_pty_data_fully_cancellable(head_bytes, cancel) {
                        Some(a) => a,
                        None => {
                            core.disable_snapshot_bypass();
                            return None;
                        }
                    },
                );
                // Suspend bypass for the MIDDLE (not `disable_snapshot_bypass`,
                // which would zero `virtual_scrollback_len` and lose the
                // HEAD's contribution to it) — `scrollback_slim` is still
                // empty (the head's own byte replay never touched it), so
                // there is nothing to fold at this transition; the fold
                // happens once, below, after the MIDDLE finishes.
                core.suspend_snapshot_bypass();
            }
            // MIDDLE: bypass is NOT enabled here (whether or not a HEAD ran
            // above), so this is a plain, full-fidelity replay — identical
            // to what the pre-D7 whole "prefix" replay did for
            // `segments[..k]`, just possibly starting partway through it.
            // D8: pass the TRUE caller target explicitly — `core` starts
            // this call at `head_rows`, not `rows`, whenever a HEAD ran
            // above, and `replay_segments`'s own "resize back to the
            // caller's target" step must land on `rows`, not `head_rows`,
            // in a SINGLE hop (see `replay_segments`'s doc for why this
            // must not be inferred from `core`'s dimensions at entry).
            let mut actions_middle =
                match core.replay_segments(middle_bytes, &middle_segments, cancel, cols, rows) {
                    Some(a) => a,
                    None => return None,
                };
            actions.append(&mut actions_middle);
            // Fold the MIDDLE's real scrollback into the SAME virtual
            // bookkeeping the bypass path uses (adding onto whatever the
            // HEAD already contributed to `virtual_scrollback_len`), so
            // `get_scrollback_length()` stays continuous across the phase
            // boundary and `enable_snapshot_bypass`'s "empty deque"
            // precondition holds.
            core.restore_bypass_invariant_after_reflow();
            core.enable_snapshot_bypass();
            // Suffix: every remaining segment already carries `(cols, rows)`
            // by construction of `k`, so no resize can occur here — feeding
            // the bytes directly (no segments) is equivalent to replaying
            // them via `replay_segments` and cheaper to compute.
            actions.extend(match core.replay_segments(suffix_bytes, &[], cancel, cols, rows) {
                Some(a) => a,
                None => {
                    // Cancelled mid-drain: leave the core consistent before
                    // discarding it via the `None` return (matches the
                    // non-split path below).
                    core.disable_snapshot_bypass();
                    return None;
                }
            });
            actions
        } else {
            if bypass_engaged {
                core.enable_snapshot_bypass();
            }
            match core.replay_segments(payload, segments, cancel, cols, rows) {
                Some(a) => a,
                None => {
                    // Cancelled mid-drain: leave the core consistent (clear the
                    // bypass) before discarding it via the `None` return so a
                    // debugger / panic handler that touches the dropped core
                    // doesn't see a half-set bypass.
                    if bypass_engaged {
                        core.disable_snapshot_bypass();
                    }
                    return None;
                }
            }
        };
        let evicted_total = core.get_scrollback_evicted_total();
        let prompt_marks = core.take_prompt_marks();
        let fold_marks = core.take_fold_marks();
        let bypass_b_mark_texts = core.take_bypass_b_mark_texts();
        // Discard any device responses (DA1 / DSR / XTWINOPS / …) generated by
        // historic queries baked into the snapshot bytes. Their originating
        // program is long gone; after the swap, the next live `take_response`
        // would otherwise pick them up and deliver a stale reply to the live
        // shell's stdin. Matches the synchronous `reset_frame_for_replay` path.
        let _ = core.take_response();
        if bypass_engaged {
            // Regression guard (review round-1 rework, finding
            // `1698d9b52a89e241`): `TerminalCore::resize` restores the
            // bypass invariant on every call made while bypass is active
            // (see that method), so `scrollback_slim` must always be empty
            // here. A future in-drain mutation path that populates
            // `scrollback_slim` WITHOUT going through `resize`'s restore
            // step would silently break the 2nd-pass merge's row-dedup
            // accounting; this makes that failure loud in tests instead.
            debug_assert!(
                core.scrollback_slim.is_empty(),
                "snapshot-replay bypass invariant violated: scrollback_slim \
                 is not empty before disable_snapshot_bypass (leaked {} rows)",
                core.scrollback_slim.len()
            );
            core.disable_snapshot_bypass();
        }
        // D3' (task0004 round-4 rework, review round-3 finding
        // `b235e4dbc61cc4ba`): `scrollback_populated` tells the caller
        // whether THIS replay actually populated `scrollback_slim` —
        // `!bypass_engaged` covers "bypass off by construction"
        // (`build_scrollback_only_from_snapshot`), "bypass downgraded for
        // this payload" (small/no stable tail, mirroring the old D6
        // row-growth guard), AND "bypass engaged for a suffix" (D1''',
        // partial — the prefix's real rows were folded into virtual
        // bookkeeping above, so `scrollback_slim` is empty end to end
        // regardless of which of these three shapes produced this result).
        Some(SnapshotReplay {
            core,
            actions,
            evicted_total,
            prompt_marks,
            fold_marks,
            bypass_b_mark_texts,
            scrollback_populated: !bypass_engaged,
        })
    }

    /// Enable the snapshot-replay bypass: subsequent `ring_push_blank`
    /// evictions skip the SlimCell intern + `scrollback_slim` push/pop work
    /// (the per-row hot loop), but still bump `virtual_scrollback_len` /
    /// `scrollback_evicted_total` so the observable bookkeeping is byte-
    /// identical to the live path on the same payload.
    ///
    /// Precondition (asserted): the scrollback deque is empty. This holds
    /// immediately after `reset()` on a freshly-constructed core (the
    /// original call site, inside `build_from_snapshot`) — AND after
    /// `restore_bypass_invariant_after_reflow` has folded a non-bypass
    /// PREFIX's real scrollback into `virtual_scrollback_len` (the D1'''
    /// round-6 rework call site inside `build_from_snapshot_inner`'s
    /// prefix/suffix split, where `virtual_scrollback_len` legitimately
    /// starts non-zero — carrying forward the prefix's real length so
    /// `get_scrollback_length()` stays continuous across the phase
    /// boundary). Only `scrollback_slim` itself is required empty here;
    /// `virtual_scrollback_len` is deliberately NOT asserted zero.
    pub(crate) fn enable_snapshot_bypass(&mut self) {
        assert!(
            self.scrollback_slim.is_empty(),
            "enable_snapshot_bypass requires an empty scrollback deque"
        );
        self.scrollback_bypass = true;
    }

    /// Disable the snapshot-replay bypass. Resets `virtual_scrollback_len`
    /// to zero so subsequent live operations on this core see the original
    /// `get_scrollback_length() == scrollback_count() as u32` semantics.
    /// `scrollback_evicted_total` is intentionally NOT touched — its
    /// monotonic semantics are part of the externally observable contract.
    pub(crate) fn disable_snapshot_bypass(&mut self) {
        self.virtual_scrollback_len = 0;
        self.scrollback_bypass = false;
        self.capture_bypass_b_marks = false;
    }

    /// Suspend the snapshot-replay bypass for the MIDDLE segment of a
    /// HEAD/MIDDLE/SUFFIX split (D7, task0001; D8, task0004) — the bypass
    /// state machine's third transition, named alongside
    /// [`Self::enable_snapshot_bypass`] / [`Self::disable_snapshot_bypass`]
    /// (task0004, review round-1 rework, finding `0e3a7dee5f50d788`).
    ///
    /// Unlike [`Self::disable_snapshot_bypass`], this does NOT zero
    /// `virtual_scrollback_len` or clear `capture_bypass_b_marks` — the
    /// HEAD's contribution to both must survive so `get_scrollback_length()`
    /// stays continuous once the MIDDLE begins folding its own real
    /// scrollback into the same bookkeeping via
    /// `restore_bypass_invariant_after_reflow`, and so a B mark emitted
    /// during the MIDDLE is captured exactly like one emitted during the
    /// HEAD or SUFFIX (see `capture_bypass_b_marks`'s doc).
    ///
    /// Precondition (asserted, debug only): `scrollback_slim` is empty —
    /// the HEAD's own byte replay never populates it for real (any resize
    /// needed to REACH the HEAD's dimensions on the fresh core happens
    /// BEFORE bypass is enabled, and is folded via
    /// `restore_bypass_invariant_after_reflow` at that point — see the
    /// `h`/`head_rows` computation in `build_from_snapshot_inner`).
    pub(crate) fn suspend_snapshot_bypass(&mut self) {
        debug_assert!(
            self.scrollback_slim.is_empty(),
            "suspend_snapshot_bypass invariant violated: the HEAD must \
             never populate real scrollback (leaked {} rows)",
            self.scrollback_slim.len()
        );
        self.scrollback_bypass = false;
    }

    /// Consume `other` and prepend its scrollback rows onto `self`,
    /// re-interning each cell's `style_id` (and `char_ref` when in
    /// `CharTable` mode) against `self.styles` / `self.chars` so the merged
    /// rows resolve against `self`'s own tables.
    ///
    /// Used by the 2nd-pass scrollback-restore worker (`tabs.rs::
    /// apply_scrollback_restore`): after `build_scrollback_only_from_snapshot`
    /// rebuilds the historical scrollback off-thread, this method merges
    /// the rebuilt scrollback into the live core. The bypass-on 1st-pass
    /// swap (see [`Self::build_from_snapshot`]) leaves
    /// `scrollback_slim` empty, and the merge restores it.
    ///
    /// FR3 trim: the caller passes `live_trim_rows`, the number of trailing
    /// rebuilt rows to drop before prepending. These correspond to scrollback
    /// rows that have already been re-emitted by the live drain between the
    /// 1st-pass swap and now; including them would duplicate rows after the
    /// merge.
    ///
    /// Preconditions:
    /// - `self.cols == other.cols` (else: log::warn + no-op; the rebuilt
    ///   rows would be the wrong width to render against this core's grid).
    ///
    /// Postconditions:
    /// - The trailing `live_trim_rows` rows of `other.scrollback_slim` /
    ///   `scrollback_wrapped` are dropped.
    /// - The remaining `other.scrollback_slim` / `scrollback_wrapped` rows
    ///   are re-interned and prepended onto `self.scrollback_slim` /
    ///   `scrollback_wrapped` (oldest-first ordering preserved).
    /// - If the combined length would exceed `self.scrollback_capacity`,
    ///   the front-most *incoming* rows are dropped (the oldest rebuilt
    ///   rows) — `self`'s existing rows are preserved (they reflect
    ///   post-bypass live drain).
    /// - `self.scrollback_evicted_total` is UNCHANGED. These rows pre-date
    ///   the bypass swap; bumping the counter would double-count against
    ///   already-emitted delta notifications (NFR5).
    /// - `other` is consumed and dropped at function end.
    ///
    /// Returns the number of rows actually inserted into `self` (the
    /// rebuilt count minus `live_trim_rows` minus any capacity-overflow
    /// drops). 0 on cols mismatch or when `live_trim_rows >= rebuilt_count`.
    pub fn merge_scrollback_from(&mut self, other: TerminalCore, live_trim_rows: usize) -> usize {
        if self.cols != other.cols {
            log::warn!(
                "merge_scrollback_from cols mismatch: self={} other={}; no-op",
                self.cols,
                other.cols
            );
            return 0;
        }
        let other_styles = other.styles;
        let other_chars = other.chars;
        let mut other_slim = other.scrollback_slim;
        let mut other_wrapped = other.scrollback_wrapped;
        // FR3: drop the trailing live-drain-collision rows before
        // re-interning so we never pay the intern cost on rows we know we
        // will throw away.
        let rebuilt_count = other_slim.len();
        if live_trim_rows >= rebuilt_count {
            // Full no-op: every row already collided with live drain.
            return 0;
        }
        let keep = rebuilt_count - live_trim_rows;
        other_slim.truncate(keep);
        other_wrapped.truncate(keep);
        // Capacity-aware pre-trim: prepend_scrollback_rows will drop the front-most
        // rows that exceed `scrollback_capacity - existing`. Doing this BEFORE the
        // re-intern loop avoids re-interning rows that get dec_ref'd immediately.
        // `live_trim_rows` (tail trim) is eviction-based; `existing` is length-based,
        // so a live ring that grew toward capacity without evicting still consumes
        // room here. The dropped rebuilt cells reference `other_styles` / `other_chars`
        // which are about to be dropped wholesale, so no dec_ref bookkeeping is needed.
        let existing = self.scrollback_slim.len();
        let room = self.scrollback_capacity.saturating_sub(existing);
        let keep_after_room = other_slim.len().min(room);
        let front_drop = other_slim.len() - keep_after_room;
        if front_drop > 0 {
            other_slim.drain(0..front_drop);
            other_wrapped.drain(0..front_drop);
        }
        // Re-intern the remaining rows. The per-cell flag dispatch mirrors
        // `release_slim_row` so refcount accounting stays symmetric across
        // the SlimCell-flag union.
        let mut reinterned_rows: Vec<Vec<crate::slim_cell::SlimCell>> =
            Vec::with_capacity(keep_after_room);
        for slim_row in other_slim.into_iter() {
            let mut new_row = Vec::with_capacity(slim_row.len());
            for slim in slim_row {
                let entry = other_styles.get_or_default(slim.style_id);
                let new_style_id = self.styles.intern(entry);
                let new_char_ref = if slim.is_char_table() {
                    let s = other_chars.get_or_default(slim.char_ref);
                    self.chars.intern(s)
                } else {
                    // INLINE_ASCII (packed UTF-8 bytes) or WIDE_CONT
                    // (unused) — copy `char_ref` as-is; CharTable is
                    // not touched.
                    slim.char_ref
                };
                new_row.push(crate::slim_cell::SlimCell {
                    char_ref: new_char_ref,
                    width: slim.width,
                    flags: slim.flags,
                    style_id: new_style_id,
                });
            }
            reinterned_rows.push(new_row);
        }
        let wrapped: Vec<bool> = other_wrapped.into_iter().collect();
        self.prepend_scrollback_rows(reinterned_rows, wrapped)
        // `other_styles` / `other_chars` drop here, releasing every
        // refcount they held over the rows we just re-interned (and over
        // the rows we trimmed before re-interning).
    }

    pub fn reset(&mut self) {
        let total = self.rows as usize * self.cols as usize;
        self.ring_cells = vec![Cell::EMPTY; total];
        self.ring_wrapped = vec![false; self.rows as usize];
        self.ring_head = 0;
        self.ring_size = self.rows as usize;
        self.scrollback_slim.clear();
        self.scrollback_wrapped.clear();
        // Full RIS-style reset clears all scrollback, so the absolute-line
        // baseline is meaningless afterwards. Resetting the counter keeps it
        // anchored to the (now empty) scrollback; consumers re-baseline off
        // the same reset notification and never see a spurious eviction
        // delta. See `scrollback_evicted_total` field docs.
        self.scrollback_evicted_total = 0;
        // Clear the snapshot-replay bypass virtual length too. The bypass
        // flag itself is NOT cleared here: `build_from_snapshot` calls
        // `reset()` *before* enabling the bypass, and a caller that toggles
        // the bypass on outside of `build_from_snapshot` is not a supported
        // shape.
        self.virtual_scrollback_len = 0;
        self.styles = StyleTable::new();
        self.chars = CharTable::new();
        self.cursor = CursorState::new();
        self.saved_cursor = None;
        // RIS clears active DECSCUSR/OSC 22 overrides; settings-derived
        // defaults survive (they are configuration, not terminal state).
        self.cursor_style_override = None;
        self.cursor_blink_override = None;
        self.modes = (1u32 << MODE_AUTO_WRAP)
            | (1u32 << MODE_CURSOR_VISIBLE)
            | (1u32 << MODE_CURSOR_BLINK)
            | (1u32 << MODE_ALTERNATE_SCROLL);
        self.tab_stops = vec![false; self.cols as usize];
        for i in (0..self.cols as usize).step_by(8) {
            self.tab_stops[i] = true;
        }
        self.overflow.clear();
        self.overflow_ridx.clear();
        self.scroll_event = None;
        // Sprint 2
        self.grapheme_buffer.clear();
        self.wrap_pending = false;
        self.last_write = None;
        self.g0_charset = 0;
        self.g1_charset = 0;
        self.active_charset = 0;
        self.scroll_region_top = 0;
        self.scroll_region_bottom = self.rows.saturating_sub(1);
        // Sprint 4
        self.response_buffer = [0u8; 64];
        self.response_len = 0;
        // Sprint 6
        self.parser.reset();
        self.mode_actions.clear();
        self.cursor_just_shown = false;
        // The line frame restarts, so any prompt marks captured before the
        // reset are meaningless. Drop them so a post-reset `take_prompt_marks`
        // returns only marks from the new stream.
        self.pending_prompt_marks.clear();
        // Same for in-flight custom-fold marks: a reset rebuilds the line
        // frame from scratch, so a `begin` captured before it would pair with
        // an `end` in an unrelated frame. Drop them.
        self.pending_fold_marks.clear();
        // Bypass B-mark text side-table: a reset starts a new parse frame, so
        // any pre-captured row texts from a previous bypass session are stale.
        self.bypass_b_mark_texts.clear();
        self.capture_bypass_b_marks = false;
        // Note: callbacks are NOT cleared on reset (terminal reset != dispose)
        self.mark_all_dirty();
        // cursor-settings-fix FR4: tell the host a full reset just ran, in
        // the same parse-order position as the cursor_style_override /
        // cursor_blink_override clearing above, so a host-side cursor-COLOR
        // override (OSC 12, tracked outside term_core) can be restored in
        // lockstep. Fired last so everything else `reset()` does has
        // already landed by the time the host's handler observes it.
        self.fire_reset_callback();
    }

    /// Take and clear the mode action queue.
    pub fn take_mode_actions(&mut self) -> Vec<u8> {
        std::mem::take(&mut self.mode_actions)
    }

    /// Record an OSC 133 prompt mark at the current absolute row. Called from
    /// the OSC handler while parsing, so each mark in a single chunk lands on
    /// the row the cursor was on *when that mark was emitted* — not the final
    /// cursor row. `kind` is the raw sub-type byte (`b'A'`..=`b'D'`); only
    /// those four reach here. Drops the oldest mark when the buffer is at
    /// `MAX_PENDING_PROMPT_MARKS` so a newline-free OSC 133 flood cannot grow
    /// it without bound (the PTY is a trust boundary).
    pub(crate) fn push_pending_prompt_mark(&mut self, kind: u8, exit_code: Option<i32>) {
        let abs_row = self.get_scrollback_length() + self.cursor.row as u32;
        let mark = PendingPromptMark {
            kind,
            abs_row,
            exit_code,
            evicted_total: self.scrollback_evicted_total,
        };
        // Cap follows MAX_PENDING_PROMPT_MARKS so an OSC 133 flood cannot grow
        // bypass_b_mark_texts without bound (PTY trust boundary, same as pending_prompt_marks).
        if self.pending_prompt_marks.len() >= MAX_PENDING_PROMPT_MARKS {
            if let Some(evicted) = self.pending_prompt_marks.pop_front() {
                self.bypass_b_mark_texts.remove(&evicted.abs_row);
            }
        }
        self.pending_prompt_marks.push_back(mark);
        // D4'''' (round-7 rework, review round-6 finding `0bed3c30e41e2389`):
        // gated on `capture_bypass_b_marks`, NOT `scrollback_bypass` — see
        // that field's doc for why. Under a bypass-engaged snapshot replay
        // (prefix OR suffix of the D1''' split, or the whole drain when
        // `k == 0`), the scrollback contents this mark's row lives in are
        // intentionally discarded, so once this row scrolls off the
        // viewport its text is irrecoverable from the bypassed store.
        // Capture the cursor row's plain text NOW, at B-mark emission time,
        // so the downstream consumer can use it instead of a scrollback
        // lookup. Only B (CommandStart) carries command text; A/C/D do not
        // need this.
        if self.capture_bypass_b_marks && kind == b'B' {
            // Secondary cap: if bypass_b_mark_texts is already at the limit
            // (possible when B marks arrive faster than pending_prompt_marks
            // fills — e.g. duplicate abs_rows), evict the oldest pending mark's
            // row from the map before inserting. This keeps the map bounded at
            // MAX_PENDING_PROMPT_MARKS entries regardless of duplication.
            if self.bypass_b_mark_texts.len() >= MAX_PENDING_PROMPT_MARKS {
                if let Some(oldest) = self.pending_prompt_marks.front() {
                    self.bypass_b_mark_texts.remove(&oldest.abs_row);
                }
            }
            let row = self.cursor.row;
            let cols = self.cols;
            let mut text = String::new();
            let mut col: u16 = 0;
            while col < cols {
                if self.get_cell_width(col, row) == 0 {
                    // Width-0: trailing half of a wide glyph — skip.
                    col += 1;
                    continue;
                }
                text.push_str(&self.get_cell_char(col, row));
                col += 1;
            }
            let text = text.trim_end().to_string();
            self.bypass_b_mark_texts.insert(abs_row, text);
        }
    }

    /// Drain the OSC 133 prompt marks captured during parsing. Native
    /// consumers call this once per pump under the same lock used to read the
    /// frame so the eviction snapshot stays consistent. Cleared by `reset()`.
    pub fn take_prompt_marks(&mut self) -> Vec<PendingPromptMark> {
        std::mem::take(&mut self.pending_prompt_marks)
            .into_iter()
            .collect()
    }

    /// Record a custom-fold mark (`OSC 777;emterm;fold;begin|end`) at the
    /// current absolute row. Called from the OSC handler while parsing, so
    /// each mark in a single chunk lands on the row the cursor was on *when
    /// that mark was emitted* — not the final cursor row, mirroring the
    /// OSC 133 prompt-mark capture. `label` is only meaningful for a `Begin`
    /// mark (the consumer applies the `"..."` fallback). Drops the oldest
    /// mark when the buffer is at `MAX_PENDING_FOLD_MARKS` so a `begin` flood
    /// cannot grow it without bound (the PTY is a trust boundary).
    pub(crate) fn push_pending_fold_mark(&mut self, kind: FoldMarkKind, label: String) {
        let abs_row = self.get_scrollback_length() + self.cursor.row as u32;
        let mark = PendingFoldMark {
            kind,
            abs_row,
            evicted_total: self.scrollback_evicted_total,
            label,
        };
        if self.pending_fold_marks.len() >= MAX_PENDING_FOLD_MARKS {
            self.pending_fold_marks.pop_front();
        }
        self.pending_fold_marks.push_back(mark);
    }

    /// Drain the custom-fold marks captured during parsing. Native consumers
    /// call this once per pump under the same lock used to read the frame so
    /// the eviction snapshot stays consistent. The consumer pairs `begin`
    /// with `end` itself. Cleared by `reset()`.
    pub fn take_fold_marks(&mut self) -> Vec<PendingFoldMark> {
        std::mem::take(&mut self.pending_fold_marks)
            .into_iter()
            .collect()
    }

    /// Drain the bypass B-mark text side-table populated during a
    /// snapshot-replay bypass. Returns a `HashMap<abs_row, command_text>`
    /// that maps each OSC 133 B mark's absolute row to the viewport row
    /// text captured at emission time. Non-empty only when
    /// `build_from_snapshot` was used (the bypass is off on the live path).
    /// Called by `build_from_snapshot` immediately before constructing
    /// `SnapshotReplay`; the consumer should prefer these texts over a
    /// scrollback lookup since the bypass does not populate scrollback.
    pub fn take_bypass_b_mark_texts(&mut self) -> std::collections::HashMap<u32, String> {
        std::mem::take(&mut self.bypass_b_mark_texts)
    }

    /// Total number of SlimCells currently held in scrollback.
    pub(crate) fn slim_cell_total(&self) -> usize {
        self.scrollback_slim.iter().map(|r| r.len()).sum()
    }

    /// Return current SlimCell scrollback statistics as a plain struct.
    /// The thin wasm wrapper converts this to a JS object via
    /// `serde-wasm-bindgen` (FR11).
    pub fn debug_slim_stats(&self) -> SlimStats {
        SlimStats {
            slim_cells: self.slim_cell_total() as u32,
            style_entries: self.styles.live_entries() as u32,
            style_bytes: self.styles.bytes_used() as u32,
            char_entries: self.chars.live_entries() as u32,
            char_bytes: self.chars.bytes_used() as u32,
        }
    }

    /// Rebuild the StyleTable / CharTable refcounts by walking the current
    /// scrollback. Returns the number of (style, char) live entries in the
    /// rebuilt tables. Used by tests and debug assertions to verify
    /// refcount integrity after operations like reflow.
    #[allow(dead_code)]
    pub(crate) fn rebuild_intern_tables_from_ring(&self) -> (usize, usize) {
        use crate::char_table::CharTable;
        use crate::slim_cell::{cell_to_slim, slim_to_cell};
        use crate::style_table::StyleTable;
        let mut styles = StyleTable::new();
        let mut chars = CharTable::new();
        for slim_row in self.scrollback_slim.iter() {
            for slim in slim_row {
                // Decompress to Cell + overflow string, re-intern into fresh tables.
                let cell = slim_to_cell(slim, &self.styles, &self.chars);
                let overflow_str = if slim.is_char_table() {
                    Some(crate::slim_cell::slim_overflow_str(slim, &self.chars).to_string())
                } else {
                    None
                };
                let _ = cell_to_slim(&cell, overflow_str.as_deref(), &mut styles, &mut chars);
            }
        }
        (styles.live_entries(), chars.live_entries())
    }

    /// Enable/disable cursor hidden→visible interrupt.
    pub fn set_cursor_show_interrupt(&mut self, enable: bool) {
        self.cursor_show_interrupt = enable;
    }

    // ── Sprint 2: Charset ───────────────────────────────

    pub fn get_g0_charset(&self) -> u8 {
        self.g0_charset
    }

    pub fn set_g0_charset(&mut self, val: u8) {
        self.g0_charset = if val <= 1 { val } else { 0 };
    }

    pub fn get_g1_charset(&self) -> u8 {
        self.g1_charset
    }

    pub fn set_g1_charset(&mut self, val: u8) {
        self.g1_charset = if val <= 1 { val } else { 0 };
    }

    pub fn get_active_charset(&self) -> u8 {
        self.active_charset
    }

    pub fn set_active_charset(&mut self, val: u8) {
        self.active_charset = if val <= 1 { val } else { 0 };
    }

    // ── Sprint 2: Scroll region ─────────────────────────

    pub fn get_scroll_region_top(&self) -> u16 {
        self.scroll_region_top
    }

    pub fn get_scroll_region_bottom(&self) -> u16 {
        self.scroll_region_bottom
    }

    pub fn set_scroll_region(&mut self, top: u16, bottom: u16) {
        let t = top.min(self.rows.saturating_sub(1));
        let b = bottom.min(self.rows.saturating_sub(1));
        if t < b {
            self.scroll_region_top = t;
            self.scroll_region_bottom = b;
        } else {
            // Invalid region: reset to full screen
            self.scroll_region_top = 0;
            self.scroll_region_bottom = self.rows.saturating_sub(1);
        }
    }

    // ── Sprint 2: Wrap pending ──────────────────────────

    pub fn get_wrap_pending(&self) -> bool {
        self.wrap_pending
    }

    pub fn set_wrap_pending(&mut self, val: bool) {
        self.wrap_pending = val;
    }

    // ── Sprint 2: Grapheme buffer ───────────────────────

    pub fn get_grapheme_buffer_len(&self) -> u32 {
        self.grapheme_buffer.len() as u32
    }

    pub fn clear_grapheme_buffer(&mut self) {
        self.grapheme_buffer.clear();
    }

    // ── Sprint 2: Internal print helpers ────────────────

    pub(crate) fn carriage_return(&mut self) {
        self.cursor.col = 0;
    }

    /// Advance cursor row. Scrolls internally if at scroll_region_bottom.
    pub(crate) fn line_feed(&mut self) {
        // A line feed always moves the cursor down (or scrolls the
        // viewport), either of which can displace the most-recently-
        // written-cell tracking used by retroactive zero-width merge.
        // Conservative: invalidate unconditionally rather than only when a
        // scroll actually occurs. Callers that immediately write a new cell
        // after wrapping (print_handler.rs) re-set this right after.
        self.last_write = None;
        if self.cursor.row >= self.scroll_region_bottom {
            self.scroll_up_internal(1);
        } else {
            self.cursor.row += 1;
        }
    }
}

// ── Structural resize-segment replay (task0004 round-4 rework, D1') ────
//
// Rounds 1-3 carried dimension changes as an in-band `OSC 777;emterm;resize;
// <cols>;<rows> BEL` byte marker (IMPLEMENTATION.md D1/D2, task0001),
// discovered at replay time by scanning the payload for that exact byte
// pattern. Every round's residual critical/high findings trace back to this
// choice: a marker embedded in the byte stream is, definitionally, also
// forgeable BY the byte stream — three attempts at filtering it out of
// PTY-sourced content each left a reconstruction path (splitting across
// filter batches, nesting inside a non-SIXEL DCS, concatenation after a
// strip pass, …).
//
// D1' removes the byte-scanning decoder entirely: dimensions are now
// supplied to replay as a structural [`ReplaySegment`] parameter (see
// [`TerminalCore::reset_and_replay_segments`] / [`TerminalCore::build_from_snapshot`]).
// No function in this module scans a byte buffer for a marker pattern any
// more, so there is nothing left for PTY output to forge.

/// D1''' (round-6 rework, superseding task0003 D6 / task0005 D5''
/// `segments_trigger_resize`): the index of the first segment in
/// `segments` such that IT and every segment after it already carries
/// `(target_cols, target_rows)` — i.e. the start of the trailing run that
/// needs no further resize once reached. Returns `segments.len()` when no
/// such run exists (including when `segments` is empty, which trivially
/// returns `0` — the whole, empty, list is "already stable" — see the
/// call site for how that degenerates to the pre-round-6 no-transition
/// fast path).
///
/// Used by [`TerminalCore::build_from_snapshot_inner`] to split a replay
/// into a (possibly empty) non-bypass PREFIX — up to and including the
/// resize that reaches the target — and a bypass-eligible SUFFIX that, by
/// this function's own definition, contains no further resize. `k == 0`
/// is exactly the case the retired `segments_trigger_resize` reported as
/// `false` (no transition anywhere in the replay): every segment already
/// opens at the target, so the whole thing is "suffix".
///
/// `clamp_resize_dims` is applied per segment here so this predicate
/// agrees with what [`TerminalCore::replay_segments`] will actually decide
/// (it clamps at the same point, D1''): an out-of-domain wire dimension
/// cannot make this predicate see a "change" that replay itself would
/// clamp away to a no-op, or vice versa.
fn stable_target_suffix_start(
    target_cols: u16,
    target_rows: u16,
    segments: &[ReplaySegment],
) -> usize {
    let target = (target_cols, target_rows);
    let mut k = segments.len();
    while k > 0 && clamp_resize_dims(segments[k - 1].cols, segments[k - 1].rows) == target {
        k -= 1;
    }
    k
}

/// D7 (task0001, NFR1-safe rescue for a resize-marker-dense tail): the size
/// of the LEADING run of `segments` that already carries a UNIFORM `(cols,
/// rows)` — the front-end complement of [`stable_target_suffix_start`]
/// (which finds the analogous TRAILING run, always uniform at the CALLER's
/// target). Returns `(h, run_rows)`: `h` segments long, all at
/// `(target_cols, run_rows)`.
///
/// D8 (task0004, review round-1 rework, finding `b21749c5f2bd1006`): unlike
/// [`stable_target_suffix_start`], the run this looks for does NOT have to
/// be at the CALLER's `target_rows` — only at `target_cols` (a column
/// change anywhere is always unsafe to fold, so a HEAD whose own columns
/// differ from the caller's target can never help; see
/// `middle_is_row_bounded`'s doc). `run_rows` is whatever row count the run
/// itself settles on, taken from `segments[0]`. This is what makes a HEAD
/// that predates a resize storm — and so sits at the storm's LARGER,
/// pre-settling size, not the storm's smaller settled target — foldable:
/// `run_rows` becomes `middle_is_row_bounded`'s safety ceiling instead of
/// the caller's `target_rows`. When the HEAD happens to already be at the
/// caller's target (`run_rows == target_rows`, every pre-D8 shape), this
/// reduces to the original `leading_target_run_len` byte-for-byte.
///
/// `build_from_snapshot_inner` calls this only on `segments[..k]` (the
/// region `stable_target_suffix_start` calls "prefix"): a large, already-
/// uniform HEAD at the very front of that region would otherwise be swept
/// into an expensive non-bypass whole-drain replay merely because SOME
/// segment further along (still before the stable tail at `k`) diverges
/// from it — the exact shape a resize-marker cluster near (but not quite
/// at) the tail produces. Returns `(0, target_cols's caller-supplied
/// target_rows)`-shaped `(0, _)` when `segments` is empty or its first
/// entry's columns differ from `target_cols` — correctly reducing to
/// "nothing to rescue" for every shape with no uniform leading run at all.
///
/// `clamp_resize_dims` is applied per segment for the same reason
/// [`stable_target_suffix_start`] applies it: agreement with what
/// `TerminalCore::replay_segments` will actually decide.
fn leading_uniform_run_len(target_cols: u16, segments: &[ReplaySegment]) -> (usize, u16) {
    let Some(first) = segments.first() else {
        return (0, 0);
    };
    let (first_cols, first_rows) = clamp_resize_dims(first.cols, first.rows);
    if first_cols != target_cols {
        return (0, 0);
    }
    let run = (first_cols, first_rows);
    let mut h = 0;
    while h < segments.len() && clamp_resize_dims(segments[h].cols, segments[h].rows) == run {
        h += 1;
    }
    (h, first_rows)
}

/// D7 safety gate: is it correct to replay [`leading_uniform_run_len`]'s
/// HEAD under bypass ahead of `middle` (the genuinely resize-needing
/// region between the head and the stable tail)? `head_rows` is the HEAD's
/// own row count — [`leading_uniform_run_len`]'s `run_rows` — NOT
/// necessarily the caller's `target_rows` (D8, task0004).
///
/// The HEAD leaves the core at `(target_cols, head_rows)` with
/// `scrollback_slim` EMPTY — bypass discards its real row content, keeping
/// only a virtual count (see `TerminalCore::scrollback_bypass`). A
/// subsequent resize can only produce a WRONG result (relative to a full,
/// non-bypass replay) if it needs to READ that discarded content, which
/// happens in exactly two cases:
///
/// - A COLUMN change: `resize_full_reflow` re-wraps EVERY row currently
///   tracked (viewport + real scrollback) to the new width — the head's
///   rows are not among what's tracked, so a column change anywhere in
///   `middle` is unconditionally rejected here (mirrors D6's treatment of
///   column changes as always needing real history).
/// - A ROW-COUNT GROW past what `middle` has itself already pushed into
///   REAL scrollback since it started: `resize_same_width`'s grow branch
///   pulls the most recently evicted rows back via
///   `scrollback_slim.pop_back()`. Since `middle` starts at EXACTLY
///   `head_rows` (inherited from the head) and this gate requires every
///   segment's (clamped) row count to stay `<= head_rows`, any grow
///   within `middle` is, by induction, recovering rows a PRIOR shrink
///   WITHIN THE SAME `middle` region already pushed there — it can never
///   reach past `middle`'s own start for the head's (discarded) rows.
///
/// Returns `false` (unsafe to fold the head in) the moment either condition
/// is violated by any segment in `middle`.
fn middle_is_row_bounded(target_cols: u16, head_rows: u16, middle: &[ReplaySegment]) -> bool {
    middle.iter().all(|s| {
        let (c, r) = clamp_resize_dims(s.cols, s.rows);
        c == target_cols && r <= head_rows
    })
}

/// Minimum suffix size (bytes), per [`stable_target_suffix_start`], below
/// which `build_from_snapshot_inner`'s D1''' prefix/suffix split is not
/// worth its own overhead (an extra `replay_segments` call plus an
/// `enable_snapshot_bypass`/`disable_snapshot_bypass` round trip) — small
/// tails (a handful of post-resize lines, as several `..._marker`
/// regression fixtures construct) fall back to the whole-drain recipe
/// unchanged, keeping those fixtures byte-identical to the pre-round-6
/// behavior. A real "ordinary switch" suffix (the pane's actual history)
/// is orders of magnitude larger than this, so the gate never affects the
/// case NFR1 targets.
const BYPASS_SUFFIX_MIN_BYTES: usize = 4096;

/// Maximum prefix size (bytes) for which `build_from_snapshot_inner`'s
/// D1''' prefix/suffix split is worth engaging (D5'''', round-7 rework,
/// review round-6 finding `e519916efd5fdc42`).
///
/// [`BYPASS_SUFFIX_MIN_BYTES`] alone gates on the SUFFIX being big enough
/// to be worth bypassing, but says nothing about the PREFIX's own cost:
/// the prefix always replays via the full, non-bypass
/// `replay_segments` — correct, but exactly as expensive per byte as the
/// whole-drain fallback this split exists to avoid. A payload that is
/// almost entirely prefix with only a small qualifying suffix (a large
/// multi-segment retained window with resizes scattered through most of
/// it, followed by a stable tail just over `BYPASS_SUFFIX_MIN_BYTES`)
/// would otherwise still engage the split: pay the full non-bypass cost
/// for the (huge) prefix as its "fast" first pass, discard that prefix's
/// real scrollback into virtual bookkeeping
/// (`restore_bypass_invariant_after_reflow`), report
/// `scrollback_populated: false`, and then pay THAT SAME non-bypass cost
/// a SECOND time when the background 2nd-pass worker
/// (`tabs.rs::apply_offthread_swap`) redoes the whole drain to actually
/// populate real scrollback — roughly doubling the work for a shape the
/// split cannot actually speed up (the ordinary-switch shape it targets
/// has a TINY prefix by construction). Below this bound, the prefix's own
/// cost is negligible either way (mirrors `tabs::OFFTHREAD_REPLAY_THRESHOLD_BYTES`'s
/// reasoning: 64 KiB is small enough that even a full non-bypass reflow
/// of it does not matter), so the split still engages whenever the
/// suffix qualifies.
///
/// D7 amendment (task0001): this bound is now checked against `middle_len`
/// (the byte span of `segments[h..k]`, per [`leading_uniform_run_len`]), not
/// the raw `split_at`/`k`-derived prefix span — when there is no rescuable
/// HEAD (`h == 0`, every pre-D7 shape), `middle_len == split_at` and this is
/// byte-identical to the original check.
const BYPASS_PREFIX_MAX_BYTES: usize = 64 * 1024;

/// Maximum number of segments the PREFIX may contain for
/// `build_from_snapshot_inner`'s D1''' prefix/suffix split to be worth
/// engaging (D5''''', round-8 rework, review round-7 finding
/// `a4f4e36fef377d05`).
///
/// [`BYPASS_PREFIX_MAX_BYTES`] bounds the prefix's total BYTE length, but a
/// prefix built from many small segments still pays one full,
/// content-preserving reflow PER SEGMENT (`replay_segments`'s per-segment
/// resize), regardless of how few total bytes those segments cover — a
/// resize storm packs up to the daemon's own `MAX_DIM_MARKERS` (24, kept as
/// a literal here — `term_core` has no dependency on the mux daemon crate;
/// see `mux_ipc::protocol::MAX_SEGMENTS`'s doc for the same duplication) worth
/// of segments into a comparatively small byte span. Bounding segment COUNT
/// independent of byte length keeps that shape from silently slipping
/// through the byte-only gate.
///
/// D7 amendment (task0001): checked against `middle_segment_count`
/// (`k - h`, per [`leading_uniform_run_len`]), not the raw `k` — when there
/// is no rescuable HEAD (`h == 0`), `middle_segment_count == k` and this is
/// byte-identical to the original check.
const BYPASS_PREFIX_MAX_SEGMENTS: usize = 24;

/// Defensive upper bound on a resize's `cols` field. Replay dimensions are
/// fed directly into `TerminalCore::resize`, which allocates
/// `(scrollback_capacity + rows) * cols` cells — an unbounded value here
/// would let a forged or corrupted dimension trigger a huge allocation.
/// Comfortably above any real terminal width.
///
/// `pub` so the daemon can validate/clamp a resize against the SAME bound at
/// the point it is applied (`MuxPane::resize`, `MuxPane::new`), so anything
/// that reaches the wire is guaranteed within what replay accepts.
pub const RESIZE_MARKER_MAX_COLS: u16 = 4096;

/// Defensive upper bound on a resize's `rows` field. See
/// [`RESIZE_MARKER_MAX_COLS`] for the rationale (both the `pub` visibility
/// and the shared-bound reasoning apply identically here).
pub const RESIZE_MARKER_MAX_ROWS: u16 = 4096;

/// Clamp a requested resize to `1..=RESIZE_MARKER_MAX_COLS` /
/// `1..=RESIZE_MARKER_MAX_ROWS`. Called at the point a resize is APPLIED
/// (`MuxPane::resize`, `MuxPane::new`) so the dimensions that reach the wire
/// — and the PTY itself — are always within replay's accepted domain. `0`
/// clamps up to `1` (a zero-sized terminal is not meaningful) rather than
/// down.
pub fn clamp_resize_dims(cols: u16, rows: u16) -> (u16, u16) {
    (
        cols.clamp(1, RESIZE_MARKER_MAX_COLS),
        rows.clamp(1, RESIZE_MARKER_MAX_ROWS),
    )
}

// ── Tests ────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── task0004 round-4 rework (D1'): structural ReplaySegment replay ────
    //
    // Rounds 1-3's `find_resize_marker` / `resize_marker_bytes` /
    // `parse_resize_marker_dims` byte-scanning decoder is GONE — replay
    // authority moved to the `ReplaySegment` parameter. The tests below
    // replace the old marker-scanning suite; AC-1's forgery tests are the
    // direct successors of that suite's intent (proving a marker-SHAPED
    // byte sequence can no longer do anything).

    /// Byte-for-byte the OLD (pre-round-4) marker wire format. Kept ONLY as
    /// adversarial test fixture data for the AC-1 forgery tests below — it
    /// is deliberately NOT wired to any production decoder any more.
    fn legacy_marker_shaped_bytes(cols: u16, rows: u16) -> Vec<u8> {
        format!("\x1b]777;emterm;resize;{cols};{rows}\x07").into_bytes()
    }

    /// AC-1: a bare marker-shaped byte sequence embedded in the replay
    /// payload, with NO segments supplied, must never change replay
    /// dimensions — not even a single reflow. `reflow_call_count` is the
    /// direct witness (per the task's test notes: `core.cols()`/`rows()`
    /// after a replay always equal the caller's target regardless of what
    /// happened mid-drain, so that alone would prove nothing; the reflow
    /// counter is what would move if the marker-shaped bytes were honored).
    ///
    /// Confirmed to fail pre-fix: against the removed byte-scanning
    /// `replay_with_resize_markers` (which called `find_resize_marker` on
    /// the raw payload), this exact input was VALID and well-formed by that
    /// decoder's own rules — it would locate the marker, call
    /// `self.resize(120, 48)` before the trailing content, and
    /// `reflow_call_count` would show 2 (the marker's resize + the final
    /// restore-to-target), not 0.
    #[test]
    fn ac1_bare_marker_shaped_bytes_never_change_replay_dimensions() {
        let mut core = TerminalCore::new(80, 24, 1000);
        let before = core.reflow_call_count;
        let mut bytes = b"before\r\n".to_vec();
        bytes.extend_from_slice(&legacy_marker_shaped_bytes(120, 48));
        bytes.extend_from_slice(b"after\r\n");
        core.reset_and_replay(&bytes); // no segments supplied
        assert_eq!(
            core.reflow_call_count - before,
            0,
            "a marker-shaped byte sequence with no segment authority must \
             never trigger a resize"
        );
        assert_eq!(core.cols(), 80);
        assert_eq!(core.rows(), 24);
        assert!(core.get_line_text(0).contains("before"));
        // The marker-shaped bytes are parsed as an ordinary (harmless,
        // unrecognized) OSC and produce no visible cell; "after" lands on
        // the very next row, exactly as if the marker text were absent.
        assert!(core.get_line_text(1).contains("after"));
    }

    /// AC-1: the SAME marker-shaped bytes, but now genuine segments ARE
    /// supplied (describing a completely different, fixed dimension) — the
    /// embedded bytes must still have zero effect; only the supplied
    /// segment's dims apply.
    #[test]
    fn ac1_marker_shaped_bytes_do_not_override_supplied_segments() {
        let mut core = TerminalCore::new(80, 24, 1000);
        let mut bytes = b"before\r\n".to_vec();
        bytes.extend_from_slice(&legacy_marker_shaped_bytes(999, 999));
        bytes.extend_from_slice(b"after\r\n");
        let segments = [ReplaySegment {
            offset: 0,
            cols: 80,
            rows: 24,
        }];
        core.reset_and_replay_segments(&bytes, &segments);
        assert_eq!(
            core.cols(),
            80,
            "dimensions must come only from the segment field"
        );
        assert_eq!(core.rows(), 24);
    }

    /// AC-1: a marker-shaped sequence "formed by concatenation" — split into
    /// two halves that are each individually harmless but literally
    /// concatenate into a complete marker byte-for-byte — still has zero
    /// effect once fed to replay as a single joined buffer with no
    /// segments. (The write-path splitting/stripping scenarios that could
    /// have produced exactly this concatenation are covered end-to-end in
    /// `mux::ipc::pty_spawn`'s AC-1 tests; this pins the term_core-level
    /// guarantee that even a PERFECTLY formed marker occurring anywhere in
    /// the byte stream is inert without segment authority.)
    #[test]
    fn ac1_marker_formed_by_concatenation_has_no_effect() {
        let full = legacy_marker_shaped_bytes(4000, 4000);
        let split = full.len() / 2;
        let mut bytes = b"before\r\n".to_vec();
        bytes.extend_from_slice(&full[..split]);
        bytes.extend_from_slice(&full[split..]);
        bytes.extend_from_slice(b"after\r\n");
        let mut core = TerminalCore::new(80, 24, 1000);
        let before = core.reflow_call_count;
        core.reset_and_replay(&bytes);
        assert_eq!(core.reflow_call_count - before, 0);
        assert_eq!(core.cols(), 80);
        assert_eq!(core.rows(), 24);
    }

    // ── clamp_resize_dims ──────────────────────────────────────────────

    #[test]
    fn clamp_resize_dims_leaves_in_range_values_untouched() {
        assert_eq!(clamp_resize_dims(80, 24), (80, 24));
        assert_eq!(
            clamp_resize_dims(RESIZE_MARKER_MAX_COLS, RESIZE_MARKER_MAX_ROWS),
            (RESIZE_MARKER_MAX_COLS, RESIZE_MARKER_MAX_ROWS)
        );
    }

    #[test]
    fn clamp_resize_dims_clamps_above_max_down_to_max() {
        assert_eq!(
            clamp_resize_dims(u16::MAX, u16::MAX),
            (RESIZE_MARKER_MAX_COLS, RESIZE_MARKER_MAX_ROWS)
        );
    }

    #[test]
    fn clamp_resize_dims_clamps_zero_up_to_one() {
        assert_eq!(clamp_resize_dims(0, 0), (1, 1));
    }

    // ── reset_and_replay_segments: structural dimension replay ────────────

    /// AC-3 (round-8 rework, review round-7 finding `01f91fe698ceb287`): a
    /// segment list whose FIRST entry does NOT start at offset 0 (the shape
    /// `ScrollbackRingBuffer::read_segments` now produces when 2+
    /// `dim_markers` entries have been evicted, leaving the leading gap
    /// deliberately unattributed) must still replay the leading gap's bytes
    /// — at the caller's TARGET dimensions (`self`'s size at the start of
    /// the call), never silently dropped.
    ///
    /// Confirmed to fail pre-fix: before this fix, the loop started at
    /// `segments[0].offset`, so `bytes[..segments[0].offset]` was never fed
    /// to any segment and the leading gap's content (`"gap-content"` below)
    /// was silently dropped — `get_line_text(0)` would not contain it.
    #[test]
    fn reset_and_replay_segments_replays_a_leading_gap_before_the_first_segment_at_target_dims() {
        let mut core = TerminalCore::new(80, 24, 1000);
        let gap = b"gap-content\r\n";
        let after = b"after-first-segment\r\n";
        let mut bytes = gap.to_vec();
        bytes.extend_from_slice(after);
        // First (and only) segment starts AFTER the gap, at a DIFFERENT
        // size than the core's target (80, 24) — so if the gap were
        // (incorrectly) fed under the segment's dims instead of being fed
        // separately first, this would still be observable as a missing
        // first line.
        let segments = [ReplaySegment {
            offset: gap.len() as u32,
            cols: 40,
            rows: 10,
        }];
        core.reset_and_replay_segments(&bytes, &segments);
        assert!(
            core.get_line_text(0).contains("gap-content"),
            "the leading gap's content must be replayed, not dropped: {:?}",
            core.get_line_text(0)
        );
        assert!(core.get_line_text(1).contains("after-first-segment"));
        // Core ends back at its ORIGINAL target dims (80, 24).
        assert_eq!(core.cols(), 80);
        assert_eq!(core.rows(), 24);
    }

    /// AC-2 / D1' equivalent of the old marker-based mid-stream resize test:
    /// a single segment transition resizes the core between the two
    /// content ranges, and the core ends back at its ORIGINAL
    /// (caller-requested) dimensions.
    #[test]
    fn reset_and_replay_segments_resizes_mid_stream_and_restores_target_dims() {
        let mut core = TerminalCore::new(80, 24, 1000);
        let before = b"before-resize\r\n";
        let after = b"after-resize\r\n";
        let mut bytes = before.to_vec();
        bytes.extend_from_slice(after);
        let segments = [
            ReplaySegment {
                offset: 0,
                cols: 80,
                rows: 24,
            },
            ReplaySegment {
                offset: before.len() as u32,
                cols: 40,
                rows: 10,
            },
        ];
        core.reset_and_replay_segments(&bytes, &segments);
        // Core ends back at the dims it was constructed with (80, 24), not
        // the mid-stream segment's (40, 10).
        assert_eq!(core.cols(), 80);
        assert_eq!(core.rows(), 24);
        assert!(
            core.get_line_text(0).contains("before-resize"),
            "content before the transition must still be present"
        );
        assert!(
            core.get_line_text(1).contains("after-resize"),
            "content after the transition must still be present"
        );
    }

    /// AC-2: content recorded under one set of dimensions is always
    /// replayed under those dimensions, including when a resize follows
    /// within what WOULD have been the (now-removed) coalescing window — a
    /// cursor-addressed write near the far edge of a wide segment only
    /// lands correctly if that segment's dims actually applied while its
    /// own bytes were fed.
    #[test]
    fn reset_and_replay_segments_honors_a_wide_dimension_segment() {
        let cols: u16 = 80;
        let rows: u16 = 24;
        let wide_cols: u16 = 2500;
        let before = b"before-resize\r\n".to_vec();
        let wide = format!("\x1b[1;{wide_cols}Hedge").into_bytes();
        let after = b"after-resize\r\n".to_vec();
        let mut bytes = before.clone();
        bytes.extend_from_slice(&wide);
        bytes.extend_from_slice(&after);
        let segments = [
            ReplaySegment {
                offset: 0,
                cols,
                rows: 40,
            },
            ReplaySegment {
                offset: before.len() as u32,
                cols: wide_cols,
                rows: 40,
            },
            ReplaySegment {
                offset: (before.len() + wide.len()) as u32,
                cols,
                rows,
            },
        ];

        let mut core = TerminalCore::new(cols, rows, 1000);
        core.reset_and_replay_segments(&bytes, &segments);

        assert_eq!(core.cols(), cols, "core must end back at target size");
        assert_eq!(core.rows(), rows);
    }

    /// review round-1 rework, finding `6ff208bbc674189c` (high) — still
    /// closed under the segment-driven replay: N consecutive segment
    /// transitions with NO bytes between them (all offsets collapse
    /// together) cost at most TWO reflows total — one for the last
    /// transition's dims right before the trailing non-empty range, and one
    /// for the mandatory final restore back to the target size — never one
    /// reflow per transition.
    #[test]
    fn replay_segments_coalesces_consecutive_empty_transitions_into_a_single_reflow() {
        let mut core = TerminalCore::new(80, 24, 1000);
        let before = core.reflow_call_count;
        let mut bytes = b"before\r\n".to_vec();
        let marker_offset = bytes.len() as u32;
        bytes.extend_from_slice(b"after\r\n");
        // A leading segment at offset 0 covers "before\r\n" at the core's
        // construction dims, then five consecutive segments, all at the
        // SAME offset (no bytes between any of them) — only the LAST one's
        // dims should ever apply to the trailing "after\r\n" range.
        let mut segments = vec![ReplaySegment {
            offset: 0,
            cols: 80,
            rows: 24,
        }];
        segments.extend(
            [(40, 10), (100, 30), (60, 20), (120, 40), (90, 25)]
                .into_iter()
                .map(|(cols, rows)| ReplaySegment {
                    offset: marker_offset,
                    cols,
                    rows,
                }),
        );
        core.reset_and_replay_segments(&bytes, &segments);
        let reflows = core.reflow_call_count - before;
        assert_eq!(
            reflows, 2,
            "a run of 5 same-offset transitions followed by ONE non-empty \
             range must reflow at most twice (last transition's dims + \
             final restore-to-target), never once per transition (which \
             would be 6 here)"
        );
        assert_eq!(core.cols(), 80, "core must end back at the target size");
        assert_eq!(core.rows(), 24);
        assert!(core.get_line_text(0).contains("before"));
        assert!(core.get_line_text(1).contains("after"));
    }

    /// Zero-reflow edge case: if a run of consecutive same-offset
    /// transitions is never followed by any bytes at all (they describe the
    /// tail of the payload), NO reflow happens for any of them.
    #[test]
    fn replay_segments_trailing_consecutive_empty_transitions_reflow_zero_times() {
        let mut core = TerminalCore::new(80, 24, 1000);
        let before = core.reflow_call_count;
        let bytes = b"only-content\r\n".to_vec();
        let tail_offset = bytes.len() as u32;
        let mut segments = vec![ReplaySegment {
            offset: 0,
            cols: 80,
            rows: 24,
        }];
        segments.extend(
            [(40, 10), (100, 30), (60, 20)]
                .into_iter()
                .map(|(cols, rows)| ReplaySegment {
                    offset: tail_offset,
                    cols,
                    rows,
                }),
        );
        core.reset_and_replay_segments(&bytes, &segments);
        assert_eq!(
            core.reflow_call_count - before,
            0,
            "a trailing run of empty transitions with nothing fed at any of \
             their sizes must cost zero reflows"
        );
        assert_eq!(core.cols(), 80);
        assert_eq!(core.rows(), 24);
        assert!(core.get_line_text(0).contains("only-content"));
    }

    /// Grid-equivalence variant: a run of consecutive same-offset
    /// transitions ending in dims (D) followed by content must produce a
    /// grid IDENTICAL to a recording containing only the SINGLE final
    /// segment (D) followed by the same content.
    #[test]
    fn replay_segments_consecutive_transitions_grid_matches_single_final_dimension_case() {
        let mut multi = TerminalCore::new(80, 24, 1000);
        let mut multi_bytes = b"before\r\n".to_vec();
        let marker_offset = multi_bytes.len() as u32;
        multi_bytes.extend_from_slice(b"after\r\n");
        let mut multi_segments = vec![ReplaySegment {
            offset: 0,
            cols: 80,
            rows: 24,
        }];
        multi_segments.extend(
            [(40, 10), (100, 30), (60, 20)]
                .into_iter()
                .map(|(cols, rows)| ReplaySegment {
                    offset: marker_offset,
                    cols,
                    rows,
                }),
        );
        multi.reset_and_replay_segments(&multi_bytes, &multi_segments);

        let mut single = TerminalCore::new(80, 24, 1000);
        let mut single_bytes = b"before\r\n".to_vec();
        let single_offset = single_bytes.len() as u32;
        single_bytes.extend_from_slice(b"after\r\n");
        let single_segments = [
            ReplaySegment {
                offset: 0,
                cols: 80,
                rows: 24,
            },
            ReplaySegment {
                offset: single_offset,
                cols: 60,
                rows: 20,
            },
        ];
        single.reset_and_replay_segments(&single_bytes, &single_segments);

        assert_eq!(grid_fingerprint(&multi), grid_fingerprint(&single));
        assert!(
            multi.get_line_text(0).contains("before"),
            "leading content must actually be fed (not silently dropped by \
             both variants, which would make this assertion vacuous)"
        );
    }

    /// AC-11: replay with NO segments (an older daemon's snapshot, or any
    /// caller with nothing to attribute) behaves as plain single-dimension
    /// replay — byte-identical to feeding the same bytes through
    /// `process_pty_data_fully` at the core's current size, with zero
    /// reflows regardless of what the bytes contain.
    #[test]
    fn ac11_no_segments_degrades_to_single_dimension_replay() {
        let payload = b"line one\r\nline two\r\n\x1b]777;emterm;resize;999;999\x07line three\r\n";

        let mut via_segments = TerminalCore::new(80, 24, 1000);
        let before = via_segments.reflow_call_count;
        via_segments.reset_and_replay_segments(payload, &[]);
        assert_eq!(via_segments.reflow_call_count - before, 0);

        let mut via_process = TerminalCore::new(80, 24, 1000);
        via_process.process_pty_data_fully(payload);

        assert_eq!(
            grid_fingerprint(&via_segments),
            grid_fingerprint(&via_process),
            "empty-segments replay must match a plain process_pty_data_fully call"
        );
    }

    // ── Off-thread snapshot replay builder (FR1/FR6/NFR2/NFR3) ──

    /// Collect the observable grid text + cursor into a comparable shape so
    /// the pure builder and the synchronous path can be asserted
    /// grid-identical. The post-replay scrollback length is intentionally
    /// excluded: per FR2, the snapshot-replay bypass leaves
    /// `scrollback_count() == 0` on the built core (contents are not
    /// repopulated), while the synchronous `reset_and_replay` path retains
    /// up to `scrollback_capacity` rows of contents. The
    /// observable bookkeeping that consumers depend on
    /// (`SnapshotReplay.evicted_total` and mark `abs_row`/`evicted_total`)
    /// is asserted separately in `test_build_from_snapshot_matches_reset_and_replay`.
    fn grid_fingerprint(core: &TerminalCore) -> (Vec<String>, u16, u16) {
        let mut rows = Vec::with_capacity(core.rows() as usize);
        for r in 0..core.rows() {
            let mut line = String::new();
            for c in 0..core.cols() {
                line.push_str(&core.get_cell_char(c, r));
            }
            rows.push(line);
        }
        (rows, core.get_cursor_col(), core.get_cursor_row())
    }

    /// TS-1: the pure builder is grid/scrollback/marks-identical to the
    /// in-place `reset_and_replay` + drain path for a representative payload
    /// (text, prompt marks, scrollback growth).
    #[test]
    fn test_build_from_snapshot_matches_reset_and_replay() {
        // Many newlines push lines into scrollback; OSC 133 A/B/C/D emit
        // prompt marks; plain text fills the grid.
        let mut payload: Vec<u8> = Vec::new();
        payload.extend_from_slice(b"\x1b]133;A\x07prompt$ \x1b]133;B\x07");
        payload.extend_from_slice(b"echo hi\x1b]133;C\x07\r\n");
        for i in 0..40u32 {
            payload.extend_from_slice(format!("line {i}\r\n").as_bytes());
        }
        payload.extend_from_slice(b"\x1b]133;D;0\x07tail here");

        // Synchronous path: an already-used core, reset_and_replay'd + drained.
        let mut sync_core = TerminalCore::new(80, 24, 1000);
        // Dirty it first so the reset inside reset_and_replay has to clean up.
        sync_core.process_pty_data_fully(b"garbage that gets reset away\r\n\r\n");
        let sync_actions = sync_core.reset_and_replay(&payload);
        let sync_evicted = sync_core.get_scrollback_evicted_total();
        let sync_prompts = sync_core.take_prompt_marks();
        let sync_folds = sync_core.take_fold_marks();

        // Off-thread builder path.
        let never = std::sync::atomic::AtomicBool::new(false);
        let built = TerminalCore::build_from_snapshot(80, 24, 1000, &payload, &[], &never)
            .expect("not cancelled");

        assert_eq!(grid_fingerprint(&built.core), grid_fingerprint(&sync_core));
        assert_eq!(built.actions, sync_actions);
        assert_eq!(built.evicted_total, sync_evicted);
        assert_eq!(built.prompt_marks, sync_prompts);
        assert_eq!(built.fold_marks, sync_folds);
    }

    /// TS-2: empty payload yields the same (empty, freshly-reset) result via
    /// both paths.
    #[test]
    fn test_build_from_snapshot_empty_payload() {
        let mut sync_core = TerminalCore::new(80, 24, 100);
        let sync_actions = sync_core.reset_and_replay(b"");
        let sync_evicted = sync_core.get_scrollback_evicted_total();
        let sync_prompts = sync_core.take_prompt_marks();
        let sync_folds = sync_core.take_fold_marks();

        let never = std::sync::atomic::AtomicBool::new(false);
        let built = TerminalCore::build_from_snapshot(80, 24, 100, b"", &[], &never)
            .expect("not cancelled");

        assert_eq!(grid_fingerprint(&built.core), grid_fingerprint(&sync_core));
        assert_eq!(built.actions, sync_actions);
        assert_eq!(built.evicted_total, sync_evicted);
        assert!(built.prompt_marks.is_empty());
        assert!(built.fold_marks.is_empty());
        assert_eq!(sync_prompts.len(), built.prompt_marks.len());
        assert_eq!(sync_folds.len(), built.fold_marks.len());
    }

    /// Regression: a DA1/DSR/XTWINOPS query baked into snapshot bytes (e.g.
    /// because some past program in the pane's scrollback wrote `\x1b[c` to
    /// `/dev/tty`) must NOT leave a residual reply in the built core's
    /// `response_buffer`. Otherwise the first live `take_response` after the
    /// swap picks it up and delivers a stale `\x1b[?65;1;4;22c` to the live
    /// shell's stdin — which zsh/zle interprets as "ESC[? prefix + literal
    /// `65;1;4;22c`", inserting the parameter tail at the prompt on the
    /// user's first keystroke after a window switch.
    #[test]
    fn test_build_from_snapshot_drains_device_response_buffer() {
        // Scrollback-shaped payload with an embedded DA1 query (`CSI c`) plus
        // a CPR query (`CSI 6 n`) — both produce buffered replies in
        // term_core when parsed.
        let mut payload = Vec::new();
        payload.extend_from_slice(b"line one\r\n");
        payload.extend_from_slice(b"\x1b[c"); // DA1 → `\x1b[?65;1;4;22c`
        payload.extend_from_slice(b"line two\r\n");
        payload.extend_from_slice(b"\x1b[6n"); // CPR → `\x1b[<r>;<c>R`

        let never = std::sync::atomic::AtomicBool::new(false);
        let built = TerminalCore::build_from_snapshot(80, 24, 100, &payload, &[], &never)
            .expect("not cancelled");

        assert_eq!(
            built.core.get_response_len(),
            0,
            "snapshot replay must discard device responses generated by \
             historic queries; residual bytes would leak as PtyInput on the \
             next live take_response after the swap"
        );
    }

    /// TS-3: the built core (and the replay result bundle) is statically
    /// movable across threads, and actually survives a round trip through a
    /// spawned worker thread.
    #[test]
    fn test_build_from_snapshot_is_send_across_threads() {
        fn assert_send<T: Send>() {}
        assert_send::<TerminalCore>();
        assert_send::<SnapshotReplay>();

        // Runtime proof: build on a worker, move the result back.
        let payload = b"hello off-thread\r\nsecond line\r\n".to_vec();
        let handle = std::thread::spawn(move || {
            static NEVER: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
            TerminalCore::build_from_snapshot(80, 24, 100, &payload, &[], &NEVER)
        });
        let built = handle
            .join()
            .expect("worker thread panicked")
            .expect("not cancelled");
        assert_eq!(built.core.get_cell_char(0, 0), "h");
        assert!(built.core.callbacks.is_none());
    }

    /// A `build_from_snapshot` whose cancel flag is already set bails at the
    /// first resume-loop boundary and returns `None` (a superseded off-thread
    /// worker discards its work). A clear flag returns `Some` as usual.
    #[test]
    fn test_build_from_snapshot_cancelled_returns_none() {
        let payload = b"row zero\r\nrow one\r\n".to_vec();

        let cancelled = std::sync::atomic::AtomicBool::new(true);
        assert!(
            TerminalCore::build_from_snapshot(80, 24, 100, &payload, &[], &cancelled).is_none(),
            "a pre-set cancel flag must abandon the build"
        );

        let live = std::sync::atomic::AtomicBool::new(false);
        assert!(
            TerminalCore::build_from_snapshot(80, 24, 100, &payload, &[], &live).is_some(),
            "a clear cancel flag must build normally"
        );
    }

    /// TS-5 (FR2): the snapshot-replay bypass intentionally does not
    /// populate scrollback content, but the returned core's
    /// `scrollback_capacity` is the caller-requested `scrollback_lines` so
    /// subsequent live PTY appends accumulate into scrollback exactly as
    /// they do today.
    #[test]
    fn test_build_from_snapshot_restores_scrollback_capacity() {
        // Build a payload that scrolls many viewport rows off the top.
        // With rows=24, 100 newlines push 100 - 24 = 76 rows past the
        // viewport. (We just need > 24 to exercise scrolling; the exact
        // number is unimportant.)
        let mut payload = Vec::new();
        for i in 0..200u32 {
            payload.extend_from_slice(format!("line {i}\r\n").as_bytes());
        }
        let scrollback_lines: u32 = 10_000;
        let never = std::sync::atomic::AtomicBool::new(false);
        let built =
            TerminalCore::build_from_snapshot(80, 24, scrollback_lines, &payload, &[], &never)
                .expect("not cancelled");
        let mut core = built.core;
        // Immediately after replay: scrollback contents are NOT populated.
        assert_eq!(
            core.scrollback_count(),
            0,
            "snapshot-replay bypass leaves scrollback contents empty (FR2)"
        );
        // But the capacity is the caller-requested value, so live appends
        // accumulate normally.
        assert_eq!(core.scrollback_capacity(), scrollback_lines);

        // Now feed N more lines through the live PTY path. The first 24 stay
        // in the viewport; subsequent lines flow into scrollback.
        let n_extra = 100u32;
        let mut extra = Vec::new();
        for i in 0..n_extra {
            extra.extend_from_slice(format!("post {i}\r\n").as_bytes());
        }
        let _ = core.process_pty_data_fully(&extra);
        // After the live drain: scrollback fills up to `min(n_extra, scrollback_lines)`.
        // (Each `\r\n` after the viewport fills generates one scrollback row.)
        let expected_scrollback = (n_extra as usize).min(scrollback_lines as usize);
        // We pushed n_extra lines; the last `rows-1` may stay in the viewport
        // depending on cursor positioning, so accept a small slack — what we
        // really want to assert is "scrollback grew, and is bounded by
        // scrollback_lines".
        assert!(
            core.scrollback_count() > 0,
            "live PTY appends must accumulate into scrollback after the bypass is disabled"
        );
        assert!(
            core.scrollback_count() <= expected_scrollback,
            "scrollback_count={} must not exceed scrollback_lines={}",
            core.scrollback_count(),
            scrollback_lines
        );
    }

    /// TS-13 (FR1 / D1 v2): `SnapshotReplay.evicted_total` produced by
    /// `build_from_snapshot` is byte-identical to the synchronous
    /// `reset_and_replay` path on the same payload + capacity, including
    /// when the payload scrolls strictly more rows than the capacity
    /// (S > small_C → evicted_total == S - small_C).
    #[test]
    fn test_build_from_snapshot_bypass_preserves_evicted_total() {
        // 24-row viewport, scrollback_lines=5 (small_C). 50 newlines push
        // 50 lines past the viewport; the first 50 - 5 = 45 of those get
        // evicted from the scrollback ring.
        let mut payload = Vec::new();
        for i in 0..50u32 {
            payload.extend_from_slice(format!("line {i}\r\n").as_bytes());
        }
        let small_c: u32 = 5;
        // Synchronous path: build the same value via reset_and_replay on a
        // fresh core of the same size.
        let mut sync_core = TerminalCore::new(80, 24, small_c);
        sync_core.reset_and_replay(&payload);
        let sync_evicted = sync_core.get_scrollback_evicted_total();

        // build_from_snapshot path: the bypass must produce the same
        // `evicted_total`.
        let never = std::sync::atomic::AtomicBool::new(false);
        let built = TerminalCore::build_from_snapshot(80, 24, small_c, &payload, &[], &never)
            .expect("not cancelled");
        assert_eq!(
            built.evicted_total, sync_evicted,
            "bypass path must preserve evicted_total byte-identically"
        );
    }

    /// TS-15 (FR1 + FR3 / D1 v2): per-mark `abs_row` and `evicted_total` on
    /// the `prompt_marks` (and `fold_marks`) drained by `build_from_snapshot`
    /// must match the synchronous `reset_and_replay` path byte-identically,
    /// including marks emitted on both sides of the `scrollback_capacity`
    /// threshold (so the saturation transition in `virtual_scrollback_len` is
    /// exercised).
    #[test]
    fn test_build_from_snapshot_bypass_preserves_mark_stamping() {
        // Construct a payload that emits OSC 133 marks BOTH before and
        // after enough scrolling to cross the scrollback_lines = small_C
        // threshold. cols=80, rows=10 viewport, small_C=5 scrollback rows.
        //
        // Sequence: first prompt mark at row 0 (no scrolling yet);
        // then 30 newlines (well past 10 viewport rows + 5 scrollback,
        // so virtual_scrollback_len saturates and evicted_total starts
        // counting); then a second prompt mark + fold begin/end.
        let mut payload = Vec::new();
        payload.extend_from_slice(b"\x1b]133;A\x07prompt1\x1b]133;B\x07ok\r\n");
        for i in 0..30u32 {
            payload.extend_from_slice(format!("line {i}\r\n").as_bytes());
        }
        payload.extend_from_slice(b"\x1b]133;A\x07prompt2\x1b]133;B\x07");
        payload.extend_from_slice(b"\x1b]777;emterm;fold;begin\x07");
        payload.extend_from_slice(b"folded text\r\n");
        payload.extend_from_slice(b"\x1b]777;emterm;fold;end\x07");
        payload.extend_from_slice(b"tail\x1b]133;D;0\x07");

        let small_c: u32 = 5;

        // Synchronous path: a fresh core of the same size, reset_and_replay,
        // then drained.
        let mut sync_core = TerminalCore::new(80, 10, small_c);
        sync_core.reset_and_replay(&payload);
        let sync_evicted = sync_core.get_scrollback_evicted_total();
        let sync_prompts = sync_core.take_prompt_marks();
        let sync_folds = sync_core.take_fold_marks();

        // build_from_snapshot path.
        let never = std::sync::atomic::AtomicBool::new(false);
        let built = TerminalCore::build_from_snapshot(80, 10, small_c, &payload, &[], &never)
            .expect("not cancelled");

        // Sanity: we exercised the saturation transition.
        assert!(
            sync_evicted > 0,
            "test payload must scroll past scrollback_lines to exercise saturation \
             (got sync_evicted={sync_evicted})"
        );
        assert!(
            !sync_prompts.is_empty(),
            "test payload must emit at least one prompt mark"
        );

        // Byte-identical mark stamping: full Vec equality covers kind, abs_row,
        // evicted_total, exit_code, label (per PendingPromptMark / PendingFoldMark
        // derive(PartialEq) shape).
        assert_eq!(built.evicted_total, sync_evicted);
        assert_eq!(built.prompt_marks, sync_prompts);
        assert_eq!(built.fold_marks, sync_folds);
    }

    /// TS-B (f2-fold-text-bypass-loss-tc): `build_from_snapshot` captures the
    /// cursor-row text of each OSC 133 B (CommandStart) mark in
    /// `SnapshotReplay.bypass_b_mark_texts` so the downstream consumer can
    /// recover the command text even though the snapshot-replay bypass
    /// discards scrollback contents.
    ///
    /// The payload emits OSC 133 A → command text → OSC 133 B, then fills
    /// the viewport with filler lines that scroll the B row into the (virtual,
    /// discarded) scrollback. We assert:
    /// 1. `bypass_b_mark_texts` is non-empty.
    /// 2. At least one entry's value contains the command text "cd /tmp".
    /// 3. The key of that entry matches the `abs_row` of one of the
    ///    `prompt_marks` with `kind == b'B'`.
    #[test]
    fn test_bypass_captures_b_mark_command_text() {
        // Construct a payload:
        //   OSC 133 A (PromptStart)
        //   "$ "  (prompt text)
        //   OSC 133 B (CommandStart)
        //   "cd /tmp"  (command text on the B-mark row — B fires AFTER the
        //               prompt, so the row already holds "$ cd /tmp" worth
        //               of content that was written before B arrived)
        //   "\r\n" + 50 filler lines that push the B row into scrollback
        let mut payload: Vec<u8> = Vec::new();
        payload.extend_from_slice(b"\x1b]133;A\x07$ ");
        // Write the command text before OSC 133 B so it is on the row at
        // the moment B fires (matching the real shell behaviour).
        payload.extend_from_slice(b"cd /tmp");
        payload.extend_from_slice(b"\x1b]133;B\x07");
        payload.extend_from_slice(b"\r\n");
        // Filler lines: more than viewport rows (24) so the B row scrolls off.
        for i in 0..60u32 {
            payload.extend_from_slice(format!("filler {i}\r\n").as_bytes());
        }

        let never = std::sync::atomic::AtomicBool::new(false);
        let built = TerminalCore::build_from_snapshot(80, 24, 100, &payload, &[], &never)
            .expect("not cancelled");

        // 1. The side-table must be non-empty.
        assert!(
            !built.bypass_b_mark_texts.is_empty(),
            "bypass_b_mark_texts should be populated for the B mark"
        );

        // 2. One entry's value must contain the command text.
        let has_command = built
            .bypass_b_mark_texts
            .values()
            .any(|v| v.contains("cd /tmp"));
        assert!(
            has_command,
            "bypass_b_mark_texts values should contain 'cd /tmp'; got: {:?}",
            built.bypass_b_mark_texts
        );

        // 3. Each key in bypass_b_mark_texts must match the abs_row of a B
        //    prompt mark drained from the same replay.
        let b_abs_rows: std::collections::HashSet<u32> = built
            .prompt_marks
            .iter()
            .filter(|m| m.kind == b'B')
            .map(|m| m.abs_row)
            .collect();
        for abs_row in built.bypass_b_mark_texts.keys() {
            assert!(
                b_abs_rows.contains(abs_row),
                "bypass_b_mark_texts key {abs_row} has no matching B prompt mark; \
                 B marks: {:?}",
                b_abs_rows
            );
        }
    }

    /// TS-5 (FR1 + NFR6 — scrollback-restore):
    /// `build_scrollback_only_from_snapshot` (bypass off) yields a core that
    /// is byte-equivalent to a synchronous `reset_and_replay` on a fresh
    /// core of the same grid / scrollback_lines.
    ///
    /// Scope: the *full* scrollback contents (slim cells + wrapped flags),
    /// `scrollback_evicted_total`, the viewport grid (via grid_fingerprint),
    /// and the drained marks must all be byte-identical. The bypass-on
    /// `build_from_snapshot` cannot satisfy this — its scrollback is empty
    /// by design — so this test is the primary unit gate that the bypass-off
    /// entry point is a drop-in replacement for the synchronous build.
    #[test]
    fn test_build_scrollback_only_from_snapshot_matches_sync_build() {
        // Construct a payload that scrolls some rows into scrollback so the
        // contents-equivalence check actually has rows to compare.
        let mut payload: Vec<u8> = Vec::new();
        payload.extend_from_slice(b"\x1b]133;A\x07$ ls\x1b]133;B\x07ok\r\n");
        for i in 0..50u32 {
            payload.extend_from_slice(format!("scroll {i}\r\n").as_bytes());
        }
        payload.extend_from_slice(b"tail");

        // Synchronous reference.
        let mut sync_core = TerminalCore::new(80, 24, 100);
        sync_core.reset_and_replay(&payload);
        let sync_evicted = sync_core.get_scrollback_evicted_total();
        let sync_prompts = sync_core.take_prompt_marks();
        let sync_folds = sync_core.take_fold_marks();
        let sync_slim: Vec<Vec<crate::slim_cell::SlimCell>> =
            sync_core.scrollback_slim.iter().cloned().collect();
        let sync_wrapped: Vec<bool> = sync_core.scrollback_wrapped.iter().copied().collect();

        // bypass-off off-thread build.
        let never = std::sync::atomic::AtomicBool::new(false);
        let built =
            TerminalCore::build_scrollback_only_from_snapshot(80, 24, 100, &payload, &[], &never)
                .expect("not cancelled");

        // Viewport grid + drained marks + evicted_total: byte-identical.
        assert_eq!(grid_fingerprint(&built.core), grid_fingerprint(&sync_core));
        assert_eq!(built.evicted_total, sync_evicted);
        assert_eq!(built.prompt_marks, sync_prompts);
        assert_eq!(built.fold_marks, sync_folds);
        // bypass_b_mark_texts is empty on the bypass-off path — the live
        // scrollback is the source of truth for B-mark texts (FR8).
        assert!(
            built.bypass_b_mark_texts.is_empty(),
            "bypass-off build must not populate bypass_b_mark_texts"
        );

        // Scrollback contents: cell-for-cell equality. This is what the
        // bypass-on path cannot deliver, and is the whole point of the
        // bypass-off entry.
        let built_slim: Vec<Vec<crate::slim_cell::SlimCell>> =
            built.core.scrollback_slim.iter().cloned().collect();
        let built_wrapped: Vec<bool> = built.core.scrollback_wrapped.iter().copied().collect();
        assert_eq!(
            built_slim.len(),
            sync_slim.len(),
            "scrollback row count must match"
        );
        assert_eq!(built_wrapped, sync_wrapped);
        // Cells: compare by decompressed text + width so we are robust to
        // any intern-id reassignment between the two cores (a same-style
        // entry may land on a different `style_id` slot because the build
        // orderings are not the same).
        for (row_idx, (a, b)) in built_slim.iter().zip(sync_slim.iter()).enumerate() {
            assert_eq!(a.len(), b.len(), "row {row_idx} length");
            for (col_idx, (sa, sb)) in a.iter().zip(b.iter()).enumerate() {
                let ca = crate::slim_cell::slim_to_cell(sa, &built.core.styles, &built.core.chars);
                let cb = crate::slim_cell::slim_to_cell(sb, &sync_core.styles, &sync_core.chars);
                assert_eq!(
                    (ca.char_data, ca.char_len, ca.width, ca.fg, ca.bg, ca.flags),
                    (cb.char_data, cb.char_len, cb.width, cb.fg, cb.bg, cb.flags),
                    "row {row_idx} col {col_idx}"
                );
            }
        }
    }

    // ── merge_scrollback_from (scrollback restore) ───────

    /// Build a TerminalCore that has `n_rows` rows of red "X" content in its
    /// scrollback, using `cols`-wide cells. Convenience for the merge tests.
    fn make_core_with_red_x_scrollback(cols: u16, n_rows: u32) -> TerminalCore {
        let mut core = TerminalCore::new(cols, 4, n_rows + 10);
        // Use a non-default fg color so each cell goes through
        // `styles.intern` with a fresh StyleEntry — that is what TS-1
        // checks (the re-intern actually rewrites style_id).
        // 0x1b 5b "31" m = set fg red.
        let mut payload: Vec<u8> = Vec::new();
        for _ in 0..n_rows {
            payload.extend_from_slice(b"\x1b[31mX\x1b[m\r\n");
        }
        core.process_pty_data_fully(&payload);
        core
    }

    /// TS-1 (FR2): `merge_scrollback_from` re-interns SlimCell ids against
    /// the receiver's tables so the merged row resolves to byte-equal style
    /// / char entries even when the two cores' intern tables differ.
    #[test]
    fn test_merge_scrollback_from_intern_rewrites_ids() {
        let mut dst = TerminalCore::new(80, 24, 100);
        // Prime dst.styles with a couple of unrelated entries so id slots
        // are unlikely to coincide with src's by accident (the test would
        // still hold under id-equality by luck).
        for ch in b"abcde" {
            let mut cell = crate::cell::Cell::EMPTY;
            cell.set_char(&(*ch as char).to_string());
            cell.fg = crate::cell::PackedColor::rgb(*ch, 0, 0);
            crate::slim_cell::cell_to_slim(&cell, None, &mut dst.styles, &mut dst.chars);
        }

        let src = make_core_with_red_x_scrollback(80, 6);
        assert!(
            !src.scrollback_slim.is_empty(),
            "src must have non-empty scrollback for the test to be meaningful"
        );

        // Snapshot src's first row's fg before consuming it.
        let src_first_row_fg = {
            let row = src.scrollback_slim.front().unwrap();
            let style = src.styles.get_or_default(row[0].style_id);
            (style.fg, style.bg, style.flags)
        };

        dst.merge_scrollback_from(src, 0);

        // The merged row should appear at the front of dst's scrollback,
        // and resolving its cell against dst.styles must yield the same
        // (fg, bg, flags) tuple — proving the style_id was re-interned
        // against dst.styles to a slot that holds an equivalent entry.
        let merged_row = dst
            .scrollback_slim
            .front()
            .expect("merge made the front non-empty");
        let merged_style = dst.styles.get_or_default(merged_row[0].style_id);
        assert_eq!(
            (merged_style.fg, merged_style.bg, merged_style.flags),
            src_first_row_fg,
            "merged row's style_id must resolve to the same fg/bg/flags via dst.styles"
        );
    }

    /// TS-2 (NFR5): the merge MUST NOT touch
    /// `self.scrollback_evicted_total`. The merged rows pre-date the bypass
    /// swap and the live-side delta accounting already covers them.
    #[test]
    fn test_merge_scrollback_from_preserves_evicted_total() {
        let mut dst = TerminalCore::new(80, 24, 4);
        // Push enough lines that the scrollback ring saturates and the
        // counter has a non-zero baseline.
        let mut bytes: Vec<u8> = Vec::new();
        for _ in 0..30 {
            bytes.extend_from_slice(b"y\r\n");
        }
        dst.process_pty_data_fully(&bytes);
        let evicted_before = dst.scrollback_evicted_total;
        assert!(
            evicted_before > 0,
            "test prerequisite: dst should have a non-zero evicted baseline"
        );

        let src = make_core_with_red_x_scrollback(80, 3);
        dst.merge_scrollback_from(src, 0);

        assert_eq!(
            dst.scrollback_evicted_total, evicted_before,
            "merge must not bump scrollback_evicted_total"
        );
    }

    /// TS-4 (FR2 defensive): cols mismatch ⇒ no-op (no merge, no panic).
    #[test]
    fn test_merge_scrollback_from_cols_mismatch_is_noop() {
        let mut dst = TerminalCore::new(80, 24, 100);
        // Push some live content so dst's scrollback has rows to compare
        // before vs. after the noop merge.
        dst.process_pty_data_fully(b"AAA\r\nBBB\r\nCCC\r\n");
        let snapshot_slim: Vec<Vec<crate::slim_cell::SlimCell>> =
            dst.scrollback_slim.iter().cloned().collect();
        let snapshot_wrapped: Vec<bool> = dst.scrollback_wrapped.iter().copied().collect();
        let snapshot_evicted = dst.scrollback_evicted_total;

        // src at a different cols width.
        let src = make_core_with_red_x_scrollback(100, 5);
        dst.merge_scrollback_from(src, 0);

        let after_slim: Vec<Vec<crate::slim_cell::SlimCell>> =
            dst.scrollback_slim.iter().cloned().collect();
        let after_wrapped: Vec<bool> = dst.scrollback_wrapped.iter().copied().collect();
        assert_eq!(
            after_slim, snapshot_slim,
            "scrollback rows must be unchanged"
        );
        assert_eq!(after_wrapped, snapshot_wrapped);
        assert_eq!(dst.scrollback_evicted_total, snapshot_evicted);
    }

    /// TS-6 (FR1 / NFR6 — primary equivalence gate): bypass-on
    /// `build_from_snapshot` + bypass-off `build_scrollback_only_from_snapshot`
    /// + `merge_scrollback_from` with `live_growth = 0` settles in a state
    /// observably equal to a single bypass-off build.
    ///
    /// This is the unit-level proof that the 2nd-pass restore worker plus
    /// the merge primitive is a drop-in replacement for the synchronous
    /// reset_and_replay path.
    #[test]
    fn test_bypass_plus_merge_equivalence() {
        // Payload that scrolls more than the viewport so scrollback has
        // non-trivial content to compare.
        let mut payload: Vec<u8> = Vec::new();
        payload.extend_from_slice(b"\x1b]133;A\x07$ ls\x1b]133;B\x07hello\r\n");
        for i in 0..40u32 {
            payload.extend_from_slice(format!("scroll {i}\r\n").as_bytes());
        }

        // Reference: single synchronous bypass-off build.
        let never = std::sync::atomic::AtomicBool::new(false);
        let reference =
            TerminalCore::build_scrollback_only_from_snapshot(80, 24, 100, &payload, &[], &never)
                .expect("reference build not cancelled");

        // Production path: bypass-on 1st-pass + bypass-off 2nd-pass + merge.
        let bypass_replay = TerminalCore::build_from_snapshot(80, 24, 100, &payload, &[], &never)
            .expect("1st-pass");
        let mut live = bypass_replay.core;
        // Bypass leaves scrollback empty by design.
        assert_eq!(live.scrollback_count(), 0);
        let rebuilt =
            TerminalCore::build_scrollback_only_from_snapshot(80, 24, 100, &payload, &[], &never)
                .expect("2nd-pass");
        // live_growth == 0: no trim necessary, merge whole rebuilt scrollback.
        live.merge_scrollback_from(rebuilt.core, 0);

        // Equivalence vs. the synchronous reference.
        assert_eq!(
            grid_fingerprint(&live),
            grid_fingerprint(&reference.core),
            "viewport grid must match"
        );
        assert_eq!(
            live.scrollback_count(),
            reference.core.scrollback_count(),
            "scrollback row count must match the synchronous reference"
        );
        // scrollback_evicted_total: both code paths produce the same
        // bypass-driven baseline (the 1st-pass produced the baseline; the
        // merge must NOT bump it). For a payload that does not saturate
        // the scrollback ring this is 0, but the contract is that the
        // counter is byte-identical to the reference regardless of saturation.
        assert_eq!(
            live.scrollback_evicted_total,
            reference.core.scrollback_evicted_total
        );
        // Cell-by-cell scrollback equality (decompressed view, robust to
        // intern slot reassignment).
        for (row_idx, (l, r)) in live
            .scrollback_slim
            .iter()
            .zip(reference.core.scrollback_slim.iter())
            .enumerate()
        {
            assert_eq!(l.len(), r.len(), "row {row_idx} length");
            for (col_idx, (sa, sb)) in l.iter().zip(r.iter()).enumerate() {
                let ca = crate::slim_cell::slim_to_cell(sa, &live.styles, &live.chars);
                let cb = crate::slim_cell::slim_to_cell(
                    sb,
                    &reference.core.styles,
                    &reference.core.chars,
                );
                assert_eq!(
                    (ca.char_data, ca.char_len, ca.width, ca.fg, ca.bg, ca.flags),
                    (cb.char_data, cb.char_len, cb.width, cb.fg, cb.bg, cb.flags),
                    "row {row_idx} col {col_idx}"
                );
            }
        }
    }

    /// review round-1 rework, finding `1698d9b52a89e241` (medium,
    /// correctness-relevant) / task0002 AC-7: a snapshot >= 64 KiB
    /// containing a ROW-COUNT-SHRINKING marker produces no
    /// duplicated / out-of-order scrollback rows and reports the SAME
    /// eviction bookkeeping as a fully synchronous (bypass-off) replay of
    /// the same payload — not merely "close".
    ///
    /// task0003 D6 update (review round-2 finding `893241823258fce3`): this
    /// payload's setup needs a GROW step before the shrink under test (to
    /// produce grown-size content for the shrink to push into scrollback).
    /// `build_from_snapshot_inner`'s D6 pre-scan sees that grow and
    /// downgrades the WHOLE replay out of the bypass fast path (see that
    /// function's doc comment), so `build_from_snapshot` alone now already
    /// returns the complete, correct scrollback for this payload — the
    /// former "manually run the 2nd-pass rebuild + merge, then assert the
    /// 1st-pass core was left empty by the bypass" recipe no longer
    /// applies (there is nothing left for a 2nd pass to add; that
    /// combination is covered instead by `test_bypass_plus_merge_equivalence`
    /// for a payload that genuinely stays bypassed). What still matters —
    /// and what this test still proves — is that the RESULT is correct: no
    /// duplicated / dropped rows and byte-identical eviction bookkeeping
    /// against the synchronous reference.
    #[test]
    fn test_bypass_plus_merge_equivalence_across_row_shrinking_resize_marker() {
        let cols: u16 = 80;
        let small_rows: u16 = 10;
        let grown_rows: u16 = 24;
        let mut payload: Vec<u8> = Vec::new();
        let mut segments: Vec<ReplaySegment> = Vec::new();
        // A handful of lines that fit within the small viewport with no
        // eviction yet, so the upcoming grow needs no history bypass would
        // have discarded.
        segments.push(ReplaySegment {
            offset: payload.len() as u32,
            cols,
            rows: small_rows,
        });
        for i in 0..5u32 {
            payload.extend_from_slice(format!("early line {i}\r\n").as_bytes());
        }
        segments.push(ReplaySegment {
            offset: payload.len() as u32,
            cols,
            rows: grown_rows,
        });
        // Bulk content at the grown size — large enough to comfortably
        // exceed 64 KiB and to populate substantial (virtual, under
        // bypass) scrollback before the shrink.
        for i in 0..3000u32 {
            payload.extend_from_slice(
                format!("grown-size scroll line {i} padded for size\r\n").as_bytes(),
            );
        }
        // The row-count-SHRINKING transition under test (AC-7): pushes the
        // rows that no longer fit the smaller viewport into scrollback —
        // exactly the content-preserving reflow finding `1698d9b52a89e241`
        // is about.
        segments.push(ReplaySegment {
            offset: payload.len() as u32,
            cols,
            rows: small_rows,
        });
        // A few lines after the shrink so the range following the
        // transition is non-empty (the resize is actually applied) — this
        // also leaves the core already at `small_rows`, matching the
        // construction / target size.
        for i in 0..5u32 {
            payload.extend_from_slice(format!("after-shrink line {i}\r\n").as_bytes());
        }
        assert!(
            payload.len() >= 64 * 1024,
            "payload must be >= 64 KiB to match AC-7's off-thread-path scenario, got {}",
            payload.len()
        );

        let never = std::sync::atomic::AtomicBool::new(false);

        // Reference: single synchronous bypass-off build.
        let reference = TerminalCore::build_scrollback_only_from_snapshot(
            cols, small_rows, 5000, &payload, &segments, &never,
        )
        .expect("reference build not cancelled");

        // Under test: `build_from_snapshot` — D6 downgrades it out of the
        // bypass for this payload (it contains a growing transition), so
        // its result alone must already match the synchronous reference.
        let bypass_replay =
            TerminalCore::build_from_snapshot(cols, small_rows, 5000, &payload, &segments, &never)
                .expect("build not cancelled");
        let live = bypass_replay.core;

        assert_eq!(
            grid_fingerprint(&live),
            grid_fingerprint(&reference.core),
            "viewport grid must match the synchronous reference"
        );
        assert_eq!(
            live.scrollback_count(),
            reference.core.scrollback_count(),
            "scrollback row count must match — no duplicated / dropped rows from a bypass leak"
        );
        assert_eq!(
            live.scrollback_evicted_total, reference.core.scrollback_evicted_total,
            "eviction bookkeeping must be byte-identical, not merely close"
        );
        for (row_idx, (l, r)) in live
            .scrollback_slim
            .iter()
            .zip(reference.core.scrollback_slim.iter())
            .enumerate()
        {
            assert_eq!(l.len(), r.len(), "row {row_idx} length");
            for (col_idx, (sa, sb)) in l.iter().zip(r.iter()).enumerate() {
                let ca = crate::slim_cell::slim_to_cell(sa, &live.styles, &live.chars);
                let cb = crate::slim_cell::slim_to_cell(
                    sb,
                    &reference.core.styles,
                    &reference.core.chars,
                );
                assert_eq!(
                    (ca.char_data, ca.char_len, ca.width, ca.fg, ca.bg, ca.flags),
                    (cb.char_data, cb.char_len, cb.width, cb.fg, cb.bg, cb.flags),
                    "row {row_idx} col {col_idx}"
                );
            }
        }
    }

    // ── task0003 AC-9 (D6, review round-2 finding `893241823258fce3`):
    // a row-count-GROWING marker inside a bypass-path (>= 64 KiB) snapshot
    // must produce the SAME viewport fingerprint as the synchronous path ──

    /// Demonstrates the divergence this fix closes: WITHOUT the D6 pre-scan
    /// (i.e. bypass stays engaged across a row-growing mid-drain resize),
    /// the grown rows come up blank instead of pulling real history —
    /// diverging from the synchronous reference. With the fix,
    /// `build_from_snapshot` (bypass path) matches
    /// `build_scrollback_only_from_snapshot` (synchronous path) exactly.
    #[test]
    fn build_from_snapshot_bypass_path_matches_sync_path_across_row_growing_marker() {
        let cols: u16 = 80;
        let small_rows: u16 = 10;
        let grown_rows: u16 = 40;
        let mut payload: Vec<u8> = Vec::new();
        // Constructed AT the grown size, so the FIRST transition (below) is
        // a shrink — history then accumulates in scrollback at the small
        // size (this is also where the bulk padding lives, so the payload
        // comfortably clears the 64 KiB off-thread bypass threshold), and
        // the SECOND transition grows back to the construction size (a
        // no-op for the implicit final restore). Nothing follows the grow,
        // so the fingerprint comparison below looks at the just-grown
        // viewport directly — content added AFTER the grow would scroll
        // the transient post-grow state out of view before this test ever
        // inspects it, hiding the divergence being tested for.
        let mut segments = vec![ReplaySegment {
            offset: 0,
            cols,
            rows: small_rows,
        }];
        // History produced at the SMALL size, comfortably more than
        // `small_rows` so there is real content sitting in scrollback for
        // the upcoming growth to pull back up into the viewport.
        for i in 0..3000u32 {
            payload.extend_from_slice(
                format!("small-size scroll line {i} padded for size\r\n").as_bytes(),
            );
        }
        // The row-count-GROWING transition under test (AC-9): the viewport
        // widens back to the construction size and should pull rows back
        // up from the scrollback history just produced above.
        segments.push(ReplaySegment {
            offset: payload.len() as u32,
            cols,
            rows: grown_rows,
        });
        assert!(
            payload.len() >= 64 * 1024,
            "payload must be >= 64 KiB to match AC-9's bypass-path scenario, got {}",
            payload.len()
        );

        let never = std::sync::atomic::AtomicBool::new(false);

        // Reference: synchronous (non-bypass) path.
        let reference = TerminalCore::build_scrollback_only_from_snapshot(
            cols, grown_rows, 5000, &payload, &segments, &never,
        )
        .expect("reference build not cancelled");

        // Under test: bypass path (`build_from_snapshot`) — D6 downgrades
        // out of the bypass for this payload because it contains a
        // row-growing transition.
        let bypass_replay =
            TerminalCore::build_from_snapshot(cols, grown_rows, 5000, &payload, &segments, &never)
                .expect("bypass-path build not cancelled");

        assert_eq!(
            grid_fingerprint(&bypass_replay.core),
            grid_fingerprint(&reference.core),
            "bypass-path viewport must match the synchronous path across a row-growing transition"
        );
    }

    /// D5'' (task0005 rework, review round-4 finding `697d8dc2b88dcddc`): a
    /// transition that changes ONLY cols (rows constant throughout,
    /// including at the implicit final restore-to-target) must ALSO
    /// downgrade out of the bypass, just like a row-growing transition —
    /// `resize_reflow` re-wraps `scrollback_slim` + the viewport together
    /// whenever EITHER dimension changes, so a cols-only resize under
    /// bypass reflows against an artificially empty scrollback and can
    /// diverge from the synchronous path exactly like the row-growth case
    /// D6 (task0003) already closed.
    ///
    /// Replaces `build_from_snapshot_stays_bypassed_for_a_cols_only_marker`
    /// (round-4 finding: that test asserted only `scrollback_count() == 0`,
    /// which is true of EVERY bypassed replay regardless of correctness and
    /// therefore could never detect this divergence — unlike the other D6
    /// tests, which compare `grid_fingerprint` against the synchronous
    /// path). This version does that comparison.
    ///
    /// One long autowrapping logical line with NO CR/LF at all, so it wraps
    /// continuously across far more physical rows than the small viewport
    /// holds — most of it must live in scrollback by the time the cols-only
    /// transition runs, which is exactly the history a cols-only reflow
    /// needs to re-split correctly at the new width.
    ///
    /// Confirmed to fail pre-fix: reverting `segments_trigger_resize` to
    /// only check row growth (the old `segments_has_row_growth` behavior)
    /// keeps this payload's replay bypassed, and the resulting
    /// `grid_fingerprint` diverges from the synchronous reference — the
    /// reflow only has the last `rows` viewport lines to re-wrap (bypass
    /// keeps `scrollback_slim` empty), not the full autowrapped history that
    /// actually produced them.
    #[test]
    fn build_from_snapshot_bypass_path_matches_sync_path_across_cols_only_marker() {
        let cols_a: u16 = 80;
        let cols_b: u16 = 40;
        let rows: u16 = 10;
        let long_line: String = (0..cols_a as usize * 1000)
            .map(|i| char::from(b'a' + (i % 26) as u8))
            .collect();
        let mut payload: Vec<u8> = long_line.into_bytes();
        assert!(
            payload.len() >= 64 * 1024,
            "must clear the off-thread bypass-path threshold (AC-7), got {}",
            payload.len()
        );

        // Only cols changes; rows stay the same throughout, including the
        // implicit final restore (both segments name the same `rows`).
        let segments = [
            ReplaySegment {
                offset: 0,
                cols: cols_a,
                rows,
            },
            ReplaySegment {
                offset: payload.len() as u32,
                cols: cols_b,
                rows,
            },
        ];
        // A little more content after the transition so the resize is
        // actually applied (`replay_segments` only resizes a segment that
        // has content to feed).
        payload.extend_from_slice(b"tail content after the cols-only resize");

        let never = std::sync::atomic::AtomicBool::new(false);
        let reference = TerminalCore::build_scrollback_only_from_snapshot(
            cols_a, rows, 5000, &payload, &segments, &never,
        )
        .expect("reference build not cancelled");
        let bypass_replay =
            TerminalCore::build_from_snapshot(cols_a, rows, 5000, &payload, &segments, &never)
                .expect("bypass-path build not cancelled");

        assert_eq!(
            grid_fingerprint(&bypass_replay.core),
            grid_fingerprint(&reference.core),
            "a cols-only transition must downgrade out of the bypass just \
             like a row-growing one — the reflow needs real scrollback \
             history to re-wrap correctly regardless of which dimension \
             changed"
        );
    }

    /// AC-5 (round-6 rework D1'''; strengthened round-7, review round-6
    /// finding `d139e51d1c8d03c8`): the ACTUAL prefix/suffix split —
    /// `build_from_snapshot`'s "ordinary switch" shape (a HEAD segment
    /// differing from the target, `MuxPane::new`'s spawn-size marker,
    /// followed by a bulk tail already AT the target — the pane's real
    /// history) — produces a grid/cursor identical to the fully
    /// synchronous reference (`build_scrollback_only_from_snapshot`) for
    /// the SAME payload, even though the split engages bypass for the
    /// suffix (verified, not assumed, per the task's AC-5 mandate).
    ///
    /// Round-7 rework: the HEAD is now longer than the viewport (spawn_rows
    /// = 24) so `restore_bypass_invariant_after_reflow`'s fold-in path
    /// actually runs (round-6's 5-line HEAD left `scrollback_slim` empty
    /// after the prefix, so that call's `leaked == 0` early-return path was
    /// the ONLY one ever exercised — the fold-in bookkeeping this split
    /// depends on was never actually tested). The comparison against the
    /// reference now also covers `evicted_total` and the full
    /// `prompt_marks` / `fold_marks` lists (previously only
    /// `grid_fingerprint`, i.e. viewport + cursor), and the payload embeds
    /// OSC 133 A/B/C/D marks in BOTH the prefix and the suffix — this is
    /// also the direct regression test for D4'''' (review round-6 finding
    /// `0bed3c30e41e2389`): the PREFIX's B mark must have its command text
    /// captured into `bypass_b_mark_texts` even though it fires before
    /// `enable_snapshot_bypass` runs.
    ///
    /// Distinguishes itself from `..._row_growing_marker` /
    /// `..._cols_only_marker` above: those fixtures have NO content after
    /// their last transition (the split has nothing to engage bypass for,
    /// `bypass_split == false`), so they exercise the "no benefit" fallback
    /// path unchanged. This fixture's tail is deliberately >=
    /// `BYPASS_SUFFIX_MIN_BYTES` so the split actually activates — confirmed
    /// below by asserting `scrollback_populated` differs between the two
    /// paths, which is the discriminating signal that this test exercises
    /// the split and not merely the pre-existing whole-drain-downgrade
    /// fallback.
    ///
    /// Confirmed to fail pre-fix (D4''''): reverting `capture_bypass_b_marks`
    /// (restoring the `scrollback_bypass`-gated capture) makes the
    /// `bypass_b_mark_texts` assertion for the PREFIX's B mark fail — that
    /// mark fires before `enable_snapshot_bypass`, so `scrollback_bypass`
    /// was still `false` at the time and the text was never captured, and
    /// by the time the consumer looks, the prefix's real scrollback row
    /// has already been folded into virtual bookkeeping and is
    /// unrecoverable.
    #[test]
    fn bypass_split_matches_reference_viewport_and_cursor_for_ordinary_switch() {
        let cols: u16 = 80;
        let spawn_rows: u16 = 24;
        let target_rows: u16 = 30;

        // HEAD: OSC 133 A/B/C/D around a command, THEN enough filler lines
        // to scroll the B-marked row well past the 24-row viewport within
        // the prefix's OWN replay — long enough that
        // `restore_bypass_invariant_after_reflow` actually folds non-empty
        // real scrollback (`leaked > 0`), not the trivial `leaked == 0`
        // early return round-6's 5-line HEAD only ever exercised.
        let mut payload: Vec<u8> = Vec::new();
        payload.extend_from_slice(b"\x1b]133;A\x07$ prefix-cmd\x1b]133;B\x07");
        payload.extend_from_slice(b"\r\n\x1b]133;C\x07prefix cmd output\r\n\x1b]133;D;0\x07");
        for i in 0..40u32 {
            payload.extend_from_slice(format!("prefix filler line {i}\r\n").as_bytes());
        }
        let head_len = payload.len() as u32;

        // TAIL: OSC 133 A/B/C/D around a DIFFERENT command plus a fold
        // begin/end pair, then the pane's real history, already at the
        // target size — comfortably over `BYPASS_SUFFIX_MIN_BYTES` (4096)
        // so the split actually engages bypass for it, and large enough
        // (with a small scrollback capacity) to force real scrolling /
        // eviction, the exact mechanism the split's viewport/cursor
        // equivalence claim depends on.
        payload.extend_from_slice(b"\x1b]133;A\x07$ suffix-cmd\x1b]133;B\x07");
        payload.extend_from_slice(b"\r\n\x1b]133;C\x07suffix cmd output\r\n");
        payload.extend_from_slice(b"\x1b]777;emterm;fold;begin\x07folded suffix text\r\n");
        payload.extend_from_slice(b"\x1b]777;emterm;fold;end\x07\x1b]133;D;0\x07");
        for i in 0..500u32 {
            payload.extend_from_slice(
                format!("pane history line {i} padded out a bit for size\r\n").as_bytes(),
            );
        }
        assert!(
            (payload.len() as u32 - head_len) >= 4096,
            "test prerequisite: the tail must clear BYPASS_SUFFIX_MIN_BYTES \
             for the split to actually engage, got {}",
            payload.len() as u32 - head_len
        );

        let segments = [
            ReplaySegment {
                offset: 0,
                cols,
                rows: spawn_rows,
            },
            ReplaySegment {
                offset: head_len,
                cols,
                rows: target_rows,
            },
        ];

        let never = std::sync::atomic::AtomicBool::new(false);
        let scrollback_lines = 200u32;
        let reference = TerminalCore::build_scrollback_only_from_snapshot(
            cols,
            target_rows,
            scrollback_lines,
            &payload,
            &segments,
            &never,
        )
        .expect("reference build not cancelled");
        let bypass_replay = TerminalCore::build_from_snapshot(
            cols,
            target_rows,
            scrollback_lines,
            &payload,
            &segments,
            &never,
        )
        .expect("bypass-path build not cancelled");

        // Discriminate: the split must actually have engaged bypass for the
        // tail — `scrollback_populated` is `false` exactly when SOME part
        // of the replay ran under bypass (real scrollback content was
        // skipped), vs the fully synchronous reference, which is always
        // `true`. Without this check, the equivalence assertions below
        // could vacuously pass because BOTH paths took the identical
        // whole-drain, non-bypass route (the `..._marker` tests' shape) —
        // this discriminator is what actually distinguishes them.
        assert!(
            !bypass_replay.scrollback_populated,
            "the split must engage bypass for the tail (scrollback_populated \
             == false) — if this is true, the split silently fell back to \
             the whole-drain path and this test is not exercising D1'''"
        );
        assert!(
            reference.scrollback_populated,
            "test prerequisite: the fully synchronous reference always \
             populates scrollback"
        );
        // Test prerequisite (round-7): the prefix must actually have
        // produced real scrollback the fold-in path had to convert — a
        // higher `evicted_total`/mark count than a trivial payload would
        // confirm the prefix's OWN reflow really scrolled past the
        // viewport, not merely that the suffix did.
        assert!(
            reference.evicted_total > 0 || reference.prompt_marks.len() >= 4,
            "test prerequisite: fixture must exercise real scrolling / \
             mark stamping in both phases"
        );

        assert_eq!(
            grid_fingerprint(&bypass_replay.core),
            grid_fingerprint(&reference.core),
            "the prefix/suffix split's viewport + cursor must match the \
             fully synchronous reference for the ordinary-switch shape"
        );
        // AC-5: the split must not silently corrupt eviction accounting or
        // the prompt/fold mark lists it hands the caller for
        // `backfill_prompt_marks` / `backfill_fold_marks` — round-6's
        // 5-line HEAD never exercised `restore_bypass_invariant_after_reflow`'s
        // non-trivial fold-in path, so a regression there could have passed
        // silently.
        assert_eq!(
            bypass_replay.evicted_total, reference.evicted_total,
            "the split must preserve evicted_total byte-identically"
        );
        assert_eq!(
            bypass_replay.prompt_marks, reference.prompt_marks,
            "the split must preserve prompt_marks byte-identically \
             (kind, abs_row, evicted_total, exit_code)"
        );
        assert_eq!(
            bypass_replay.fold_marks, reference.fold_marks,
            "the split must preserve fold_marks byte-identically"
        );

        // D4'''' regression: both the PREFIX's and the SUFFIX's B mark
        // command text must be captured into `bypass_b_mark_texts` — the
        // prefix's fires before `enable_snapshot_bypass` runs, so gating
        // capture on `scrollback_bypass` alone (round-6) missed it.
        let b_marks: Vec<_> = bypass_replay
            .prompt_marks
            .iter()
            .filter(|m| m.kind == b'B')
            .collect();
        assert_eq!(
            b_marks.len(),
            2,
            "test prerequisite: exactly one B mark in the prefix and one \
             in the suffix, got {b_marks:?}"
        );
        let prefix_b_abs_row = b_marks[0].abs_row;
        let suffix_b_abs_row = b_marks[1].abs_row;
        assert!(
            bypass_replay
                .bypass_b_mark_texts
                .get(&prefix_b_abs_row)
                .is_some_and(|text| text.contains("prefix-cmd")),
            "the PREFIX's B mark command text must be captured into \
             bypass_b_mark_texts even though it fires before bypass is \
             enabled — got {:?}",
            bypass_replay.bypass_b_mark_texts.get(&prefix_b_abs_row)
        );
        assert!(
            bypass_replay
                .bypass_b_mark_texts
                .get(&suffix_b_abs_row)
                .is_some_and(|text| text.contains("suffix-cmd")),
            "the SUFFIX's B mark command text must be captured into \
             bypass_b_mark_texts — got {:?}",
            bypass_replay.bypass_b_mark_texts.get(&suffix_b_abs_row)
        );
    }

    /// D5''''' (round-8 rework, review round-7 findings `7c70216c5a5d5c24`
    /// / `a4f4e36fef377d05`): a deterministic, non-ignored regression
    /// pinning the split gate's boundary — the ONLY prior assertion of
    /// this shape lived in an `#[ignore]`d timing bench
    /// (`bench.rs::large_prefix_small_suffix_bench_does_not_engage_the_split`),
    /// so deleting the gate left the normal `cargo test` run green.
    ///
    /// Pins the side of the boundary where the split must NOT engage: a
    /// prefix JUST OVER `BYPASS_PREFIX_MAX_BYTES` (64 KiB), even paired
    /// with a suffix that clears (and dominates) `BYPASS_SUFFIX_MIN_BYTES`.
    /// The companion test below pins the other side (prefix AT/under the
    /// byte bound, with a dominating suffix, DOES engage).
    ///
    /// Confirmed to fail pre-fix: deleting the `split_at <=
    /// BYPASS_PREFIX_MAX_BYTES` gate entirely makes this payload's split
    /// engage (since the suffix here easily clears `BYPASS_SUFFIX_MIN_BYTES`
    /// and dominates the prefix), so `scrollback_populated` comes back
    /// `false` and the assertion below fails.
    #[test]
    fn prefix_just_over_the_byte_bound_does_not_engage_the_split() {
        let cols: u16 = 100;
        let target_rows: u16 = 30;
        let other_rows: u16 = 24;

        let filler = b"prefix line padded out a bit for size\r\n";
        let mut payload: Vec<u8> = Vec::new();
        let mut segments = vec![ReplaySegment {
            offset: 0,
            cols,
            rows: other_rows,
        }];
        while payload.len() <= 64 * 1024 {
            payload.extend_from_slice(filler);
        }
        let prefix_len = payload.len();
        assert!(
            prefix_len > 64 * 1024,
            "test prerequisite: prefix must exceed BYPASS_PREFIX_MAX_BYTES"
        );

        // Suffix: at the target dims, large enough to DOMINATE the prefix
        // (bigger than it) — isolates the BYTE-BOUND gate as the ONLY
        // reason this must not split (the "suffix dominates" and
        // "segment count" gates are satisfied here).
        segments.push(ReplaySegment {
            offset: payload.len() as u32,
            cols,
            rows: target_rows,
        });
        while payload.len() - prefix_len <= prefix_len {
            payload.extend_from_slice(filler);
        }

        let never = std::sync::atomic::AtomicBool::new(false);
        let replay = TerminalCore::build_from_snapshot(
            cols,
            target_rows,
            10_000,
            &payload,
            &segments,
            &never,
        )
        .expect("not cancelled");
        assert!(
            replay.scrollback_populated,
            "a prefix just over BYPASS_PREFIX_MAX_BYTES must not engage the \
             split, even with a dominating suffix — scrollback_populated \
             must be true (whole-drain fallback), not false"
        );
    }

    /// D5''''' companion (see the test above): a prefix AT/under
    /// `BYPASS_PREFIX_MAX_BYTES`, with a suffix that DOMINATES it (at least
    /// as large) and a segment count within `BYPASS_PREFIX_MAX_SEGMENTS`,
    /// DOES engage the split — pins the other side of the boundary so a
    /// future change cannot silently turn the gate into "never split".
    #[test]
    fn prefix_at_the_byte_bound_with_a_dominating_suffix_engages_the_split() {
        let cols: u16 = 100;
        let target_rows: u16 = 30;
        let other_rows: u16 = 24;

        let filler = b"prefix line padded out a bit for size\r\n";
        let mut payload: Vec<u8> = Vec::new();
        let mut segments = vec![ReplaySegment {
            offset: 0,
            cols,
            rows: other_rows,
        }];
        // A small prefix, comfortably under the byte bound and with a
        // single segment (comfortably under BYPASS_PREFIX_MAX_SEGMENTS).
        while payload.len() < 8192 {
            payload.extend_from_slice(filler);
        }
        let prefix_len = payload.len();
        assert!(prefix_len <= 64 * 1024, "test prerequisite");

        // Suffix: at the target dims, larger than the prefix (dominates)
        // and clears BYPASS_SUFFIX_MIN_BYTES.
        segments.push(ReplaySegment {
            offset: payload.len() as u32,
            cols,
            rows: target_rows,
        });
        while payload.len() - prefix_len < prefix_len.max(4096) * 2 {
            payload.extend_from_slice(filler);
        }

        let never = std::sync::atomic::AtomicBool::new(false);
        let replay = TerminalCore::build_from_snapshot(
            cols,
            target_rows,
            10_000,
            &payload,
            &segments,
            &never,
        )
        .expect("not cancelled");
        assert!(
            !replay.scrollback_populated,
            "a small prefix with a dominating suffix (both within the byte \
             and segment-count bounds) must engage the split — \
             scrollback_populated must be false"
        );
    }

    /// D5''''' (round-8 rework, review round-7 finding `a4f4e36fef377d05`):
    /// the EXACT repro the finding names — a prefix AT the byte bound (64
    /// KiB) with only a small suffix JUST over `BYPASS_SUFFIX_MIN_BYTES`
    /// (4096, ~16x smaller than the prefix) must NOT engage the split. The
    /// byte-only gate alone (`split_at <= BYPASS_PREFIX_MAX_BYTES` AND
    /// `suffix_len >= BYPASS_SUFFIX_MIN_BYTES`) is satisfied by this
    /// payload — only the NEW "suffix must dominate" requirement
    /// (`suffix_len >= split_at`) rejects it.
    ///
    /// Confirmed to fail pre-fix: reverting to the byte-only gate (dropping
    /// `suffix_len >= split_at`) makes this payload's split engage —
    /// `scrollback_populated` comes back `false` and the assertion below
    /// fails.
    #[test]
    fn prefix_at_byte_bound_with_non_dominating_suffix_does_not_engage_the_split() {
        let cols: u16 = 100;
        let target_rows: u16 = 30;
        let other_rows: u16 = 24;

        let filler = b"prefix line padded out a bit for size\r\n";
        let mut payload: Vec<u8> = Vec::new();
        let mut segments = vec![ReplaySegment {
            offset: 0,
            cols,
            rows: other_rows,
        }];
        // Prefix: right at BYPASS_PREFIX_MAX_BYTES (64 KiB) — never add a
        // chunk that would push it OVER the bound.
        while payload.len() + filler.len() <= 64 * 1024 {
            payload.extend_from_slice(filler);
        }
        let prefix_len = payload.len();
        assert!(
            prefix_len <= 64 * 1024,
            "test prerequisite: prefix must clear BYPASS_PREFIX_MAX_BYTES"
        );

        // Suffix: just over BYPASS_SUFFIX_MIN_BYTES (4096) — clears the
        // absolute floor but is dwarfed by the prefix (does NOT dominate).
        segments.push(ReplaySegment {
            offset: payload.len() as u32,
            cols,
            rows: target_rows,
        });
        while payload.len() - prefix_len < 4096 + 512 {
            payload.extend_from_slice(filler);
        }
        let suffix_len = payload.len() - prefix_len;
        assert!(
            suffix_len >= 4096,
            "test prerequisite: suffix must clear BYPASS_SUFFIX_MIN_BYTES"
        );
        assert!(
            suffix_len < prefix_len,
            "test prerequisite: suffix must NOT dominate the prefix"
        );

        let never = std::sync::atomic::AtomicBool::new(false);
        let replay = TerminalCore::build_from_snapshot(
            cols,
            target_rows,
            10_000,
            &payload,
            &segments,
            &never,
        )
        .expect("not cancelled");
        assert!(
            replay.scrollback_populated,
            "a prefix at the byte bound with a small, non-dominating \
             suffix must not engage the split — scrollback_populated must \
             be true (whole-drain fallback), not false"
        );
    }

    /// D5''''' (round-8 rework, review round-7 finding `a4f4e36fef377d05`):
    /// the segment-count bound — a prefix with MORE than
    /// `BYPASS_PREFIX_MAX_SEGMENTS` segments must not engage the split,
    /// even when its byte length is tiny and its suffix dominates (both of
    /// which are otherwise sufficient).
    ///
    /// Confirmed to fail pre-fix: dropping the `k <=
    /// BYPASS_PREFIX_MAX_SEGMENTS` gate makes this payload's split engage —
    /// `scrollback_populated` comes back `false` and the assertion below
    /// fails.
    #[test]
    fn prefix_with_too_many_segments_does_not_engage_the_split_regardless_of_byte_length() {
        let cols: u16 = 100;
        let target_rows: u16 = 30;
        let other_rows: u16 = 24;

        let segment_count = BYPASS_PREFIX_MAX_SEGMENTS + 1;
        let filler = b"tiny\r\n";
        let mut payload: Vec<u8> = Vec::new();
        let mut segments = Vec::with_capacity(segment_count + 1);
        for i in 0..segment_count {
            segments.push(ReplaySegment {
                offset: payload.len() as u32,
                cols,
                rows: if i % 2 == 0 {
                    other_rows
                } else {
                    other_rows + 1
                },
            });
            payload.extend_from_slice(filler);
        }
        let prefix_len = payload.len();
        assert!(
            prefix_len <= 64 * 1024,
            "test prerequisite: prefix must clear BYPASS_PREFIX_MAX_BYTES \
             despite the excess segment count"
        );

        // Suffix: large enough to dominate the (tiny) prefix and clear
        // BYPASS_SUFFIX_MIN_BYTES.
        segments.push(ReplaySegment {
            offset: payload.len() as u32,
            cols,
            rows: target_rows,
        });
        let suffix_filler = b"suffix line padded out a bit for size\r\n";
        while payload.len() - prefix_len < 8192 {
            payload.extend_from_slice(suffix_filler);
        }

        let never = std::sync::atomic::AtomicBool::new(false);
        let replay = TerminalCore::build_from_snapshot(
            cols,
            target_rows,
            10_000,
            &payload,
            &segments,
            &never,
        )
        .expect("not cancelled");
        assert!(
            replay.scrollback_populated,
            "a prefix with more than BYPASS_PREFIX_MAX_SEGMENTS segments \
             must not engage the split, even with a tiny byte length and a \
             dominating suffix — scrollback_populated must be true, not \
             false"
        );
    }

    // ── task0001 D7: head/middle/tail generalization (resize-marker-dense
    // tail rescue) ──────────────────────────────────────────────────────

    /// AC-1 / AC-4 (task0001, D7): reproduces the measured "resize-marker-
    /// dense scrollback tail" shape (see task0001's SPEC.md References) — a
    /// large HEAD already at the target dims, followed by a dense cluster
    /// of resize markers (dims oscillating between two values BELOW the
    /// target, never reaching or exceeding it — the D7 safety condition)
    /// whose own content is tiny, followed by a small qualifying tail back
    /// at the target. Confirms:
    ///
    /// - AC-1: the split engages (`scrollback_populated == false`) even
    ///   though the RAW "prefix" (head + cluster, the only definition of
    ///   that word before D7) exceeds every pre-D7 threshold: `k` (head +
    ///   cluster segment count) exceeds `BYPASS_PREFIX_MAX_SEGMENTS`, the
    ///   raw prefix byte length exceeds `BYPASS_PREFIX_MAX_BYTES`, and the
    ///   suffix does not dominate that raw prefix. D7 recognizes that only
    ///   the small MIDDLE (the cluster itself) needs non-bypass fidelity —
    ///   the HEAD can fold into bypass too.
    /// - AC-4: the resulting viewport + cursor are byte-identical to the
    ///   fully synchronous reference (`build_scrollback_only_from_snapshot`)
    ///   for the SAME payload, `evicted_total` matches, and
    ///   `scrollback_populated` carries its usual meaning (`false` for the
    ///   bypass-engaged replay, `true` for the reference) — this fix does
    ///   not special-case this shape into a different, non-equivalent fast
    ///   path.
    ///
    /// Confirmed to fail pre-fix (D7): reverting to `stable_target_suffix_start`
    /// alone (no `h` / `leading_target_run_len`) makes `k` land at the
    /// START of the HEAD (the head's own segment no longer counts as
    /// "stable" once ANY later segment diverges, under the old trailing-
    /// run-only definition) — `k` exceeds `BYPASS_PREFIX_MAX_SEGMENTS`, the
    /// raw prefix byte length exceeds `BYPASS_PREFIX_MAX_BYTES`, the suffix
    /// does not dominate it, `bypass_split` is `false`, and
    /// `scrollback_populated` comes back `true`.
    #[test]
    fn head_plus_marker_cluster_engages_the_split_and_matches_reference() {
        let cols: u16 = 80;
        let target_rows: u16 = 30;
        let cluster_rows_a: u16 = 24;
        let cluster_rows_b: u16 = 26;

        // HEAD: a single large segment already AT the target — the bulk of
        // the pane's real history, well over BYPASS_PREFIX_MAX_BYTES (64
        // KiB) on its own so the OLD whole-prefix byte gate would already
        // reject this shape.
        let head_filler = b"head history line padded out a bit for size\r\n";
        let mut payload: Vec<u8> = Vec::new();
        let mut segments = vec![ReplaySegment {
            offset: 0,
            cols,
            rows: target_rows,
        }];
        while payload.len() <= 96 * 1024 {
            payload.extend_from_slice(head_filler);
        }
        let head_len = payload.len();
        assert!(
            head_len > 64 * 1024,
            "test prerequisite: HEAD alone must exceed BYPASS_PREFIX_MAX_BYTES"
        );

        // MIDDLE: a dense cluster of exactly BYPASS_PREFIX_MAX_SEGMENTS
        // resize markers, dims oscillating between two values below the
        // target, tiny content between them — combined with the HEAD
        // segment, `k` (head + cluster) is BYPASS_PREFIX_MAX_SEGMENTS + 1,
        // one past the OLD gate's bound, while the cluster's OWN segment
        // count (`middle_segment_count`) sits exactly AT the bound.
        let cluster_segment_count = BYPASS_PREFIX_MAX_SEGMENTS;
        let cluster_filler = b"x\r\n";
        for i in 0..cluster_segment_count {
            segments.push(ReplaySegment {
                offset: payload.len() as u32,
                cols,
                rows: if i % 2 == 0 {
                    cluster_rows_a
                } else {
                    cluster_rows_b
                },
            });
            payload.extend_from_slice(cluster_filler);
        }
        let middle_len = payload.len() - head_len;
        assert!(
            middle_len <= 64 * 1024,
            "test prerequisite: the cluster's OWN content must clear \
             BYPASS_PREFIX_MAX_BYTES for D7 to have anything to rescue"
        );

        // TAIL: small, just over BYPASS_SUFFIX_MIN_BYTES, back at the
        // target — dominates the MIDDLE (D7's new gate) but NOT the raw
        // head+cluster prefix (the OLD gate's dominance check).
        segments.push(ReplaySegment {
            offset: payload.len() as u32,
            cols,
            rows: target_rows,
        });
        let tail_filler = b"tail history line padded out a bit for size\r\n";
        while payload.len() - head_len - middle_len < 4096 + 512 {
            payload.extend_from_slice(tail_filler);
        }
        let suffix_len = payload.len() - head_len - middle_len;
        assert!(
            suffix_len >= middle_len,
            "test prerequisite: suffix must dominate the MIDDLE (D7's gate)"
        );
        assert!(
            suffix_len < head_len + middle_len,
            "test prerequisite: suffix must NOT dominate the raw head+cluster \
             prefix (the OLD gate's dominance check must still fail here)"
        );

        let never = std::sync::atomic::AtomicBool::new(false);
        let scrollback_lines = 10_000u32;
        let bypass_replay = TerminalCore::build_from_snapshot(
            cols,
            target_rows,
            scrollback_lines,
            &payload,
            &segments,
            &never,
        )
        .expect("bypass-path build not cancelled");
        let reference = TerminalCore::build_scrollback_only_from_snapshot(
            cols,
            target_rows,
            scrollback_lines,
            &payload,
            &segments,
            &never,
        )
        .expect("reference build not cancelled");

        // AC-1: the split must engage despite the raw head+cluster prefix
        // failing every pre-D7 threshold.
        assert!(
            !bypass_replay.scrollback_populated,
            "AC-1: the split must engage (scrollback_populated == false) for \
             a resize-marker-dense tail behind an already-at-target HEAD — \
             got scrollback_populated == true (D7 did not rescue this shape)"
        );
        assert!(
            reference.scrollback_populated,
            "test prerequisite: the fully synchronous reference always \
             populates scrollback"
        );

        // AC-4: viewport + cursor equivalence with the fully synchronous
        // reference, and matching evicted_total (the split must not merely
        // look right on the grid while silently corrupting bookkeeping).
        assert_eq!(
            grid_fingerprint(&bypass_replay.core),
            grid_fingerprint(&reference.core),
            "AC-4: the head/middle/tail split's viewport + cursor must match \
             the fully synchronous reference for the marker-cluster shape"
        );
        assert_eq!(
            bypass_replay.evicted_total, reference.evicted_total,
            "AC-4: the split must preserve evicted_total byte-identically"
        );
    }

    /// AC-1 (task0004, D8, review round-1 rework finding `b21749c5f2bd1006`):
    /// the mirror of `head_plus_marker_cluster_engages_the_split_and_matches_reference`
    /// above, but in the direction the SPEC's own root cause actually
    /// takes: the settled target is the SMALLER (status-bar-visible) size,
    /// so the pre-settling HEAD — and roughly half the resize-marker
    /// cluster — sits at rows ABOVE the target, not below it. A large HEAD
    /// already at a size N > target_rows (predating the storm), followed
    /// by a dense cluster oscillating between N and the (smaller) target,
    /// whose own content is tiny, followed by a small qualifying tail back
    /// at the target. Confirms:
    ///
    /// - AC-1: the split engages (`scrollback_populated == false`) even
    ///   though the RAW "prefix" (head + cluster) exceeds every pre-D7
    ///   threshold, exactly as the below-target companion test proves for
    ///   the other direction — `leading_uniform_run_len`'s HEAD need not
    ///   open at `target_rows` itself, only at SOME uniform size the
    ///   cluster stays within.
    /// - AC-4-equivalence: the resulting viewport + cursor and
    ///   `evicted_total` match the fully synchronous reference.
    ///
    /// Confirmed to fail pre-fix (task0004 D8): reverting to
    /// `leading_target_run_len` (HEAD must open AT `target_rows`) makes `h`
    /// land at `0` for this shape (the HEAD opens at `N`, not
    /// `target_rows`), so `middle_segment_count` and the raw prefix byte
    /// length are exactly the head+cluster totals again — both exceed
    /// their bounds, the tail does not dominate them, `bypass_split` is
    /// `false`, and `scrollback_populated` comes back `true`.
    #[test]
    fn head_plus_marker_cluster_above_target_engages_the_split_and_matches_reference() {
        let cols: u16 = 80;
        let target_rows: u16 = 24;
        let head_rows: u16 = 30;
        let cluster_rows_below: u16 = 24;

        // HEAD: a single large segment already at `head_rows` (N), the size
        // the pane held BEFORE the resize storm — well over
        // `BYPASS_PREFIX_MAX_BYTES` (64 KiB) on its own so the OLD
        // whole-prefix byte gate would already reject this shape.
        let head_filler = b"head history line padded out a bit for size\r\n";
        let mut payload: Vec<u8> = Vec::new();
        let mut segments = vec![ReplaySegment {
            offset: 0,
            cols,
            rows: head_rows,
        }];
        while payload.len() <= 96 * 1024 {
            payload.extend_from_slice(head_filler);
        }
        let head_len = payload.len();
        assert!(
            head_len > 64 * 1024,
            "test prerequisite: HEAD alone must exceed BYPASS_PREFIX_MAX_BYTES"
        );

        // MIDDLE: a dense cluster of exactly BYPASS_PREFIX_MAX_SEGMENTS
        // resize markers, dims oscillating between the target (BELOW
        // head_rows) and head_rows itself — never exceeding head_rows, the
        // D8 safety condition — ending on `head_rows` (NOT the target) so
        // the "settling" drop happens right at the k boundary, not inside
        // the cluster.
        let cluster_segment_count = BYPASS_PREFIX_MAX_SEGMENTS;
        let cluster_filler = b"x\r\n";
        for i in 0..cluster_segment_count {
            segments.push(ReplaySegment {
                offset: payload.len() as u32,
                cols,
                rows: if i % 2 == 0 {
                    cluster_rows_below
                } else {
                    head_rows
                },
            });
            payload.extend_from_slice(cluster_filler);
        }
        assert_eq!(
            cluster_segment_count % 2,
            0,
            "test prerequisite: an even cluster length ends on the odd \
             index (head_rows), not the target"
        );
        let middle_len = payload.len() - head_len;
        assert!(
            middle_len <= 64 * 1024,
            "test prerequisite: the cluster's OWN content must clear \
             BYPASS_PREFIX_MAX_BYTES for D8 to have anything to rescue"
        );

        // TAIL: small, just over BYPASS_SUFFIX_MIN_BYTES, back at the
        // (smaller) target — dominates the MIDDLE (D8's gate) but NOT the
        // raw head+cluster prefix (the OLD gate's dominance check).
        segments.push(ReplaySegment {
            offset: payload.len() as u32,
            cols,
            rows: target_rows,
        });
        let tail_filler = b"tail history line padded out a bit for size\r\n";
        while payload.len() - head_len - middle_len < 4096 + 512 {
            payload.extend_from_slice(tail_filler);
        }
        let suffix_len = payload.len() - head_len - middle_len;
        assert!(
            suffix_len >= middle_len,
            "test prerequisite: suffix must dominate the MIDDLE (D8's gate)"
        );
        assert!(
            suffix_len < head_len + middle_len,
            "test prerequisite: suffix must NOT dominate the raw head+cluster \
             prefix (the OLD gate's dominance check must still fail here)"
        );

        let never = std::sync::atomic::AtomicBool::new(false);
        let scrollback_lines = 10_000u32;
        let bypass_replay = TerminalCore::build_from_snapshot(
            cols,
            target_rows,
            scrollback_lines,
            &payload,
            &segments,
            &never,
        )
        .expect("bypass-path build not cancelled");
        let reference = TerminalCore::build_scrollback_only_from_snapshot(
            cols,
            target_rows,
            scrollback_lines,
            &payload,
            &segments,
            &never,
        )
        .expect("reference build not cancelled");

        // AC-1: the split must engage despite the raw head+cluster prefix
        // failing every pre-D7 threshold, in the ABOVE-target direction.
        assert!(
            !bypass_replay.scrollback_populated,
            "AC-1: the split must engage (scrollback_populated == false) for \
             a resize-marker-dense tail oscillating ABOVE the settled target \
             behind an already-larger HEAD — got scrollback_populated == \
             true (D8 did not rescue the above-target direction)"
        );
        assert!(
            reference.scrollback_populated,
            "test prerequisite: the fully synchronous reference always \
             populates scrollback"
        );

        // AC-4-equivalence: viewport + cursor equivalence with the fully
        // synchronous reference, and matching evicted_total.
        assert_eq!(
            grid_fingerprint(&bypass_replay.core),
            grid_fingerprint(&reference.core),
            "the head/middle/tail split's viewport + cursor must match the \
             fully synchronous reference for the above-target marker-cluster \
             shape"
        );
        assert_eq!(
            bypass_replay.evicted_total, reference.evicted_total,
            "the split must preserve evicted_total byte-identically"
        );
    }

    /// AC-2 (task0004, review round-1 rework finding `6a02ed7e1b606588`):
    /// reproduces the finding's own example — a small target HEAD, a small
    /// column-change MIDDLE, and a large target TAIL — a shape that engaged
    /// the split BEFORE D7 was introduced. Confirms `head_fold_safe == false`
    /// (a column change is always unsafe to fold, regardless of row bounds)
    /// degrades `h` to `0` rather than abandoning the split entirely: with
    /// `h == 0`, `middle_len == split_at` and `middle_segment_count == k`,
    /// so the pre-D7 gates (byte length, segment count, suffix dominance)
    /// are evaluated on exactly the shape they always were, and the split
    /// still engages.
    ///
    /// Confirmed to fail pre-fix (task0004 D9): with `head_fold_safe` ANDed
    /// directly into `bypass_split` (the pre-task0004 code), `h == 1`
    /// (the small target HEAD) makes `middle_is_row_bounded` reject the
    /// column-changing MIDDLE, `head_fold_safe` is `false`,
    /// `bypass_split` is `false` (the WHOLE split, not just the fold, is
    /// abandoned), and `scrollback_populated` comes back `true` — the full
    /// non-bypass drain this fix exists to avoid for a shape that used to
    /// be fast.
    #[test]
    fn column_change_middle_degrades_head_fold_but_still_engages_the_split() {
        let cols: u16 = 80;
        let other_cols: u16 = 100;
        let target_rows: u16 = 24;

        // HEAD: small, already at the target.
        let head_filler = b"head\r\n";
        let mut payload: Vec<u8> = Vec::new();
        let mut segments = vec![ReplaySegment {
            offset: 0,
            cols,
            rows: target_rows,
        }];
        while payload.len() < 2048 {
            payload.extend_from_slice(head_filler);
        }
        let head_len = payload.len();

        // MIDDLE: a single COLUMN-CHANGING segment (same row count, but
        // different columns) — always unsafe to fold behind a bypassed
        // HEAD regardless of any row-count reasoning (see
        // `middle_is_row_bounded`'s doc).
        segments.push(ReplaySegment {
            offset: payload.len() as u32,
            cols: other_cols,
            rows: target_rows,
        });
        let middle_filler = b"mid\r\n";
        while payload.len() - head_len < 2048 {
            payload.extend_from_slice(middle_filler);
        }
        let middle_len = payload.len() - head_len;
        let prefix_len = head_len + middle_len;
        assert!(
            prefix_len <= 64 * 1024,
            "test prerequisite: the combined head+middle prefix must clear \
             BYPASS_PREFIX_MAX_BYTES for the pre-D7 gates to accept it"
        );

        // TAIL: back at the target, dominating the combined prefix.
        segments.push(ReplaySegment {
            offset: payload.len() as u32,
            cols,
            rows: target_rows,
        });
        let tail_filler = b"tail history line padded out a bit for size\r\n";
        while payload.len() - prefix_len < 8192 {
            payload.extend_from_slice(tail_filler);
        }
        let suffix_len = payload.len() - prefix_len;
        assert!(
            suffix_len >= prefix_len,
            "test prerequisite: the tail must dominate the combined prefix"
        );

        let never = std::sync::atomic::AtomicBool::new(false);
        let replay = TerminalCore::build_from_snapshot(
            cols,
            target_rows,
            10_000,
            &payload,
            &segments,
            &never,
        )
        .expect("not cancelled");
        let reference = TerminalCore::build_scrollback_only_from_snapshot(
            cols,
            target_rows,
            10_000,
            &payload,
            &segments,
            &never,
        )
        .expect("reference build not cancelled");

        // AC-2: the split must still engage — the column change degrades
        // the HEAD fold, not the whole split.
        assert!(
            !replay.scrollback_populated,
            "AC-2: a column-change MIDDLE behind a small target HEAD, with a \
             dominating target TAIL, must still engage the split \
             (scrollback_populated == false) — an unsafe HEAD fold must \
             degrade `h` to 0, not abandon the split entirely"
        );
        assert_eq!(
            grid_fingerprint(&replay.core),
            grid_fingerprint(&reference.core),
            "the degraded (h == 0) split must match the fully synchronous \
             reference for the column-change-in-the-middle shape"
        );
        assert_eq!(
            replay.evicted_total, reference.evicted_total,
            "the split must preserve evicted_total byte-identically"
        );
    }

    /// AC-5 (task0001, D7): confirms the pre-existing "ordinary" (no HEAD)
    /// segment-count boundary behavior is unchanged by D7 — a prefix with
    /// EXACTLY `BYPASS_PREFIX_MAX_SEGMENTS` (24) segments, small in bytes,
    /// paired with a dominating suffix, still engages the split (companion
    /// to the pre-existing
    /// `prefix_with_too_many_segments_does_not_engage_the_split_regardless_of_byte_length`,
    /// which pins the "25 segments rejects" side of the same boundary).
    /// Distinguishes itself from
    /// `head_plus_marker_cluster_engages_the_split_and_matches_reference`
    /// above: `h == 0` here (the first segment does not open at the
    /// target), so `middle_segment_count == k` exactly — this is the direct
    /// regression guard that D7's `h` / `middle_segment_count` computation
    /// reduces to the pre-D7 `k` byte-for-byte when there is no HEAD to
    /// rescue, not merely "close" — i.e. D7 does not accidentally widen
    /// acceptance beyond what AC-1's specific marker-cluster case requires.
    #[test]
    fn prefix_at_the_segment_count_bound_with_a_dominating_suffix_engages_the_split_no_head() {
        let cols: u16 = 100;
        let target_rows: u16 = 30;
        let other_rows: u16 = 24;

        let filler = b"tiny\r\n";
        let mut payload: Vec<u8> = Vec::new();
        let mut segments = Vec::with_capacity(BYPASS_PREFIX_MAX_SEGMENTS + 1);
        for i in 0..BYPASS_PREFIX_MAX_SEGMENTS {
            segments.push(ReplaySegment {
                offset: payload.len() as u32,
                cols,
                rows: if i % 2 == 0 {
                    other_rows
                } else {
                    other_rows + 1
                },
            });
            payload.extend_from_slice(filler);
        }
        let prefix_len = payload.len();
        assert!(
            prefix_len <= 64 * 1024,
            "test prerequisite: prefix must clear BYPASS_PREFIX_MAX_BYTES"
        );

        segments.push(ReplaySegment {
            offset: payload.len() as u32,
            cols,
            rows: target_rows,
        });
        let suffix_filler = b"suffix line padded out a bit for size\r\n";
        while payload.len() - prefix_len < 8192 {
            payload.extend_from_slice(suffix_filler);
        }

        let never = std::sync::atomic::AtomicBool::new(false);
        let replay = TerminalCore::build_from_snapshot(
            cols,
            target_rows,
            10_000,
            &payload,
            &segments,
            &never,
        )
        .expect("not cancelled");
        assert!(
            !replay.scrollback_populated,
            "a prefix with EXACTLY BYPASS_PREFIX_MAX_SEGMENTS segments (no \
             HEAD), a tiny byte length, and a dominating suffix must still \
             engage the split — scrollback_populated must be false, not \
             true (D7 must not accidentally narrow the pre-existing \
             boundary)"
        );
    }

    /// AC-6 (task0001, NFR1 regression guard, D7): a genuinely large,
    /// content-heavy MIDDLE (not a resize-marker cluster — real scrollback
    /// content, a single segment) behind an already-at-target HEAD must
    /// still NOT engage the split, even though a HEAD is present and D7's
    /// safety condition (`middle_is_row_bounded`) holds for it. Proves D7's
    /// head/middle generalization does not widen acceptance for the shape
    /// `BYPASS_PREFIX_MAX_BYTES` exists to reject (see that constant's doc
    /// history) merely because a small HEAD happens to precede it — only
    /// the MIDDLE's own size decides, exactly as D2 (IMPLEMENTATION.md)
    /// requires.
    ///
    /// Confirmed to fail if D7 folded the HEAD in without ALSO re-checking
    /// the MIDDLE's own size against `BYPASS_PREFIX_MAX_BYTES` (i.e. if it
    /// only subtracted the head's byte length from the raw `split_at`
    /// without gating `middle_len` itself): this ~96 KiB of real content
    /// would otherwise engage the split behind the small head, paying the
    /// 2nd-pass worker's full non-bypass cost a second time (NFR1).
    #[test]
    fn head_plus_large_content_heavy_middle_does_not_engage_the_split() {
        let cols: u16 = 80;
        let target_rows: u16 = 30;
        let other_rows: u16 = 24;

        // HEAD: small, already at the target.
        let head_filler = b"head line\r\n";
        let mut payload: Vec<u8> = Vec::new();
        let mut segments = vec![ReplaySegment {
            offset: 0,
            cols,
            rows: target_rows,
        }];
        while payload.len() < 512 {
            payload.extend_from_slice(head_filler);
        }
        let head_len = payload.len();

        // MIDDLE: a SINGLE segment (well under BYPASS_PREFIX_MAX_SEGMENTS)
        // but genuinely large in bytes — real scrollback content, not a
        // sparse marker cluster.
        segments.push(ReplaySegment {
            offset: payload.len() as u32,
            cols,
            rows: other_rows,
        });
        let middle_filler = b"real scrollback content line padded a bit\r\n";
        while payload.len() - head_len <= 96 * 1024 {
            payload.extend_from_slice(middle_filler);
        }
        let middle_len = payload.len() - head_len;
        assert!(
            middle_len > 64 * 1024,
            "test prerequisite: the MIDDLE alone must exceed \
             BYPASS_PREFIX_MAX_BYTES"
        );

        // TAIL: dominates the MIDDLE, back at the target.
        segments.push(ReplaySegment {
            offset: payload.len() as u32,
            cols,
            rows: target_rows,
        });
        let tail_filler = b"tail line padded out a bit for size\r\n";
        let tail_start = payload.len();
        while payload.len() - tail_start < middle_len + 4096 {
            payload.extend_from_slice(tail_filler);
        }

        let never = std::sync::atomic::AtomicBool::new(false);
        let replay = TerminalCore::build_from_snapshot(
            cols,
            target_rows,
            10_000,
            &payload,
            &segments,
            &never,
        )
        .expect("not cancelled");
        assert!(
            replay.scrollback_populated,
            "AC-6: a genuinely large, content-heavy MIDDLE behind a HEAD \
             must NOT engage the split, even though the HEAD itself is \
             safe to fold — scrollback_populated must be true (whole-drain \
             fallback), not false"
        );
    }

    /// Regression pin (task0003, prior-feature review round-2 critical
    /// `5c6ae6b507b6f638`): the empty-MIDDLE degradation contract
    /// (IMPLEMENTATION.md `Empty-MIDDLE degradation contract`) for the
    /// `h == k` shape — the ENTIRE pre-suffix region is a single uniform
    /// run at `(target_cols, R)` with `R` strictly above the caller's
    /// target `rows`, so `leading_uniform_run_len` reports a candidate `h`
    /// covering the whole region (`candidate_h == k`). Folding that
    /// candidate in would leave an empty MIDDLE — `replay_segments` early-
    /// returns for empty `segments` WITHOUT its final "resize back to the
    /// caller's target" hop — so `candidate_h < k` must reject the fold and
    /// degrade `h` to `0` (the pre-D7 whole-prefix path). The region is
    /// sized well over `BYPASS_PREFIX_MAX_BYTES` (64 KiB) so that, once
    /// degraded, it also fails the ordinary (no-HEAD) split gates on its
    /// own merits and falls all the way back to the fully synchronous
    /// whole-drain replay — the same replay the reference build performs.
    ///
    /// Confirmed to fail pre-fix (before the `candidate_h < k` guard
    /// existed): with the guard removed, `candidate_safe` accepts `h == k`
    /// here (`candidate_h > 0`, `candidate_rows(R=30) >= rows(24)`, and
    /// `middle_is_row_bounded` vacuously holds over the empty
    /// `segments[candidate_h..k]` slice) regardless of the region's own
    /// byte length — folding an empty MIDDLE in skips the
    /// `BYPASS_PREFIX_MAX_BYTES` check entirely (it is evaluated against
    /// `middle_len`, which is `0` for `h == k`). The HEAD then replays
    /// under bypass at `head_rows == 30` and the core is never resized
    /// back down — the round-2 finding's own empirically-confirmed
    /// failure: requested `(80, 24)`, got `(80, 30)`, with
    /// `scrollback_populated` coming back `false` (the split wrongly
    /// reports itself engaged) instead of matching the reference build's
    /// `true`. This test built that exact shape against the pre-fix guard
    /// (locally, not committed) and observed precisely that divergence
    /// before confirming the guard below prevents it.
    #[test]
    fn whole_prefix_uniform_head_run_degrades_empty_middle_fold_and_matches_reference() {
        let cols: u16 = 80;
        let target_rows: u16 = 24;
        let head_run_rows: u16 = 30; // R, strictly above target_rows

        // Pre-suffix region: a SINGLE segment (k == 1), uniform at
        // (cols, head_run_rows) — trivially a "single uniform run" whose
        // leading_uniform_run_len candidate covers the whole region
        // (candidate_h == k == 1). Sized well over BYPASS_PREFIX_MAX_BYTES
        // (64 KiB) so the degraded (h == 0) path's own gate rejects it too.
        let head_filler = b"head history line padded out a bit for size\r\n";
        let mut payload: Vec<u8> = Vec::new();
        let mut segments = vec![ReplaySegment {
            offset: 0,
            cols,
            rows: head_run_rows,
        }];
        while payload.len() <= 96 * 1024 {
            payload.extend_from_slice(head_filler);
        }
        let head_len = payload.len();
        assert!(
            head_len > 64 * 1024,
            "test prerequisite: the pre-suffix region must exceed \
             BYPASS_PREFIX_MAX_BYTES so the degraded path also rejects it"
        );

        // Qualifying stable target-dims suffix, just over
        // BYPASS_SUFFIX_MIN_BYTES.
        segments.push(ReplaySegment {
            offset: payload.len() as u32,
            cols,
            rows: target_rows,
        });
        let tail_filler = b"tail history line padded out a bit for size\r\n";
        while payload.len() - head_len < 4096 + 512 {
            payload.extend_from_slice(tail_filler);
        }

        let never = std::sync::atomic::AtomicBool::new(false);
        let scrollback_lines = 10_000u32;
        let bypass_replay = TerminalCore::build_from_snapshot(
            cols,
            target_rows,
            scrollback_lines,
            &payload,
            &segments,
            &never,
        )
        .expect("bypass-path build not cancelled");
        let reference = TerminalCore::build_scrollback_only_from_snapshot(
            cols,
            target_rows,
            scrollback_lines,
            &payload,
            &segments,
            &never,
        )
        .expect("reference build not cancelled");

        // AC-1: the built core must land at the CALLER-requested (cols,
        // rows) — not at the HEAD run's R.
        assert_eq!(
            bypass_replay.core.cols(),
            cols,
            "AC-1: the degraded empty-MIDDLE fold must still resize back to \
             the caller's target column count"
        );
        assert_eq!(
            bypass_replay.core.rows(),
            target_rows,
            "AC-1: the degraded empty-MIDDLE fold must still resize back to \
             the caller's target row count, not stay at the HEAD run's R \
             (the round-2 finding's failure: requested (80, 24), got \
             (80, 30))"
        );

        // AC-2: scrollback_populated must match the reference non-bypass
        // build of the identical payload/segments.
        assert_eq!(
            bypass_replay.scrollback_populated, reference.scrollback_populated,
            "AC-2: scrollback_populated must match the reference build's \
             value for this shape"
        );
        assert!(
            reference.scrollback_populated,
            "test prerequisite: the fully synchronous reference always \
             populates scrollback"
        );
    }

    // ── Grid construction ────────────────────────────────

    #[test]
    fn test_grid_new_80x24() {
        let core = TerminalCore::new(80, 24, 0);
        assert_eq!(core.cols(), 80);
        assert_eq!(core.rows(), 24);
        // All cells should be empty spaces
        for row in 0..24 {
            assert!(core.is_line_empty(row));
        }
    }

    // ── Cell set/get round-trip ──────────────────────────

    #[test]
    fn test_set_get_cell_ascii() {
        let mut core = TerminalCore::new(80, 24, 0);
        core.set_cell(0, 0, "A", 1, 0, 0, 0, 0, 0, 0, 0, 0, 0);
        assert_eq!(core.get_cell_char(0, 0), "A");
        assert_eq!(core.get_cell_width(0, 0), 1);
    }

    #[test]
    fn test_set_get_cell_cjk() {
        let mut core = TerminalCore::new(80, 24, 0);
        core.set_cell(5, 3, "漢", 2, 0, 0, 0, 0, 0, 0, 0, 0, 0);
        assert_eq!(core.get_cell_char(5, 3), "漢");
        assert_eq!(core.get_cell_width(5, 3), 2);
    }

    #[test]
    fn test_set_get_cell_ascii_fast() {
        let mut core = TerminalCore::new(80, 24, 0);
        core.set_cell_ascii(10, 5, b'Z', 2, 100, 200, 50, 0, 0, 0, 0, 0);
        assert_eq!(core.get_cell_char(10, 5), "Z");
        assert_eq!(core.get_cell_width(10, 5), 1);
        let fg = core.get_cell_fg(10, 5);
        assert_eq!(fg >> 24, 2); // tag = RGB
        assert_eq!((fg >> 16) & 0xFF, 100); // r
    }

    #[test]
    fn test_set_get_cell_with_attrs() {
        let mut core = TerminalCore::new(80, 24, 0);
        // Set with RGB fg, indexed bg, bold+italic
        core.set_cell(
            0,
            0,
            "X",
            1,
            2,
            255,
            128,
            64,
            1,
            42,
            0,
            0,
            STYLE_BOLD | STYLE_ITALIC,
        );
        assert_eq!(core.get_cell_char(0, 0), "X");
        let fg = core.get_cell_fg(0, 0);
        assert_eq!(PackedColor::from_u32(fg), PackedColor::rgb(255, 128, 64));
        let bg = core.get_cell_bg(0, 0);
        assert_eq!(PackedColor::from_u32(bg), PackedColor::indexed(42));
        assert_eq!(core.get_cell_flags(0, 0), STYLE_BOLD | STYLE_ITALIC);
    }

    // ── Out-of-bounds ────────────────────────────────────

    #[test]
    fn test_oob_write_noop() {
        let mut core = TerminalCore::new(80, 24, 0);
        // Should not panic
        core.set_cell(80, 0, "X", 1, 0, 0, 0, 0, 0, 0, 0, 0, 0);
        core.set_cell(0, 24, "X", 1, 0, 0, 0, 0, 0, 0, 0, 0, 0);
    }

    #[test]
    fn test_oob_read_default() {
        let core = TerminalCore::new(80, 24, 0);
        assert_eq!(core.get_cell_char(80, 0), " ");
        assert_eq!(core.get_cell_width(0, 24), 1);
        assert_eq!(core.get_cell_fg(100, 100), 0);
    }

    // ── Line operations ──────────────────────────────────

    #[test]
    fn test_clear_line() {
        let mut core = TerminalCore::new(80, 24, 0);
        core.set_cell(0, 0, "A", 1, 0, 0, 0, 0, 0, 0, 0, 0, 0);
        core.set_cell(1, 0, "B", 1, 0, 0, 0, 0, 0, 0, 0, 0, 0);
        core.clear_line(0);
        assert_eq!(core.get_cell_char(0, 0), " ");
        assert_eq!(core.get_cell_char(1, 0), " ");
        assert!(core.is_line_empty(0));
    }

    #[test]
    fn test_clear_line_range() {
        let mut core = TerminalCore::new(80, 24, 0);
        for col in 0..10 {
            core.set_cell(col, 0, "X", 1, 0, 0, 0, 0, 0, 0, 0, 0, 0);
        }
        core.clear_line_range(0, 3, 7);
        assert_eq!(core.get_cell_char(2, 0), "X");
        assert_eq!(core.get_cell_char(3, 0), " ");
        assert_eq!(core.get_cell_char(6, 0), " ");
        assert_eq!(core.get_cell_char(7, 0), "X");
    }

    #[test]
    fn test_get_line_text() {
        let mut core = TerminalCore::new(10, 1, 0);
        core.set_cell(0, 0, "H", 1, 0, 0, 0, 0, 0, 0, 0, 0, 0);
        core.set_cell(1, 0, "i", 1, 0, 0, 0, 0, 0, 0, 0, 0, 0);
        // Width-0 placeholder (e.g., second cell of wide char)
        let text = core.get_line_text(0);
        assert!(text.starts_with("Hi"));
    }

    #[test]
    fn test_get_line_text_skips_width0() {
        let mut core = TerminalCore::new(10, 1, 0);
        core.set_cell(0, 0, "漢", 2, 0, 0, 0, 0, 0, 0, 0, 0, 0);
        // Set width=0 placeholder at col 1
        core.set_cell(1, 0, "", 0, 0, 0, 0, 0, 0, 0, 0, 0, 0);
        let text = core.get_line_text(0);
        // Should have "漢" followed by spaces, not the empty placeholder
        assert!(text.starts_with("漢"));
        assert!(!text.contains('\0'));
    }

    #[test]
    fn test_is_line_empty() {
        let mut core = TerminalCore::new(80, 24, 0);
        assert!(core.is_line_empty(0));
        core.set_cell(5, 0, "A", 1, 0, 0, 0, 0, 0, 0, 0, 0, 0);
        assert!(!core.is_line_empty(0));
    }

    // ── Row operations ───────────────────────────────────

    #[test]
    fn test_shift_rows_up() {
        let mut core = TerminalCore::new(10, 5, 0);
        // Set identifiable content on each row
        for row in 0..5 {
            core.set_cell(0, row, &format!("{row}"), 1, 0, 0, 0, 0, 0, 0, 0, 0, 0);
        }
        core.shift_rows_up(0, 4, 2);
        // Row 0 should now have what was row 2
        assert_eq!(core.get_cell_char(0, 0), "2");
        assert_eq!(core.get_cell_char(0, 1), "3");
        assert_eq!(core.get_cell_char(0, 2), "4");
        // Bottom rows should be cleared
        assert_eq!(core.get_cell_char(0, 3), " ");
        assert_eq!(core.get_cell_char(0, 4), " ");
    }

    #[test]
    fn test_shift_rows_down() {
        let mut core = TerminalCore::new(10, 5, 0);
        for row in 0..5 {
            core.set_cell(0, row, &format!("{row}"), 1, 0, 0, 0, 0, 0, 0, 0, 0, 0);
        }
        core.shift_rows_down(0, 4, 2);
        // Top rows should be cleared
        assert_eq!(core.get_cell_char(0, 0), " ");
        assert_eq!(core.get_cell_char(0, 1), " ");
        // Original rows shifted down
        assert_eq!(core.get_cell_char(0, 2), "0");
        assert_eq!(core.get_cell_char(0, 3), "1");
        assert_eq!(core.get_cell_char(0, 4), "2");
    }

    #[test]
    fn test_copy_row() {
        let mut core = TerminalCore::new(10, 5, 0);
        core.set_cell(0, 0, "X", 1, 2, 255, 0, 0, 0, 0, 0, 0, STYLE_BOLD);
        core.set_line_wrapped(0, true);
        core.copy_row(0, 3);
        assert_eq!(core.get_cell_char(0, 3), "X");
        assert_eq!(core.get_cell_flags(0, 3), STYLE_BOLD);
        assert!(core.get_line_wrapped(3));
    }

    #[test]
    fn test_fill_row_default() {
        let mut core = TerminalCore::new(10, 5, 0);
        core.set_cell(0, 2, "Z", 1, 0, 0, 0, 0, 0, 0, 0, 0, 0);
        core.fill_row_default(2);
        assert!(core.is_line_empty(2));
    }

    // ── Resize ───────────────────────────────────────────

    #[test]
    fn test_resize_grow_cols() {
        let mut core = TerminalCore::new(10, 5, 0);
        core.set_cell(5, 0, "A", 1, 0, 0, 0, 0, 0, 0, 0, 0, 0);
        core.resize(20, 5);
        assert_eq!(core.cols(), 20);
        assert_eq!(core.get_cell_char(5, 0), "A");
    }

    #[test]
    fn test_resize_shrink_cols() {
        let mut core = TerminalCore::new(10, 5, 0);
        core.set_cell(8, 0, "A", 1, 0, 0, 0, 0, 0, 0, 0, 0, 0);
        core.resize(5, 5);
        assert_eq!(core.cols(), 5);
        // Col 8 should be gone, reading it via get_cell_char returns default
        assert_eq!(core.get_cell_char(8, 0), " ");
    }

    #[test]
    fn test_resize_grow_shrink_rows() {
        let mut core = TerminalCore::new(10, 5, 0);
        core.set_cell(0, 0, "A", 1, 0, 0, 0, 0, 0, 0, 0, 0, 0);

        // Grow
        core.resize(10, 10);
        assert_eq!(core.rows(), 10);
        assert_eq!(core.get_cell_char(0, 0), "A");

        // Shrink
        core.resize(10, 3);
        assert_eq!(core.rows(), 3);
        assert_eq!(core.get_cell_char(0, 0), "A");
    }

    // ── Reset ────────────────────────────────────────────

    #[test]
    fn test_reset() {
        let mut core = TerminalCore::new(80, 24, 0);
        core.set_cell(5, 5, "X", 1, 0, 0, 0, 0, 0, 0, 0, 0, STYLE_BOLD);
        core.set_cursor(40, 12);
        core.set_mode(MODE_BRACKETED_PASTE, true);
        core.reset();

        assert_eq!(core.get_cursor_col(), 0);
        assert_eq!(core.get_cursor_row(), 0);
        assert!(core.get_mode(MODE_AUTO_WRAP));
        assert!(!core.get_mode(MODE_BRACKETED_PASTE));
        assert!(core.is_line_empty(5));
    }

    /// AC-5: `reset()` fires the GUI-agnostic "full reset occurred" signal
    /// (`TerminalCallbacks::on_reset`), the mechanism a host uses to restore
    /// a theme-side OSC 12 cursor-color override (cursor-settings-fix FR4).
    #[test]
    fn test_reset_fires_on_reset_callback() {
        use std::sync::Arc;
        use std::sync::atomic::{AtomicUsize, Ordering};

        struct Recorder(Arc<AtomicUsize>);
        impl crate::callbacks::TerminalCallbacks for Recorder {
            fn on_osc(&self, _action_type: u8, _data: &str) {}
            fn on_apc(&self, _data: &[u8]) {}
            fn on_dcs(&self, _data: &[u8]) {}
            fn on_bell(&self) {}
            fn on_device_response(&self, _data: &[u8]) {}
            fn on_reset(&self) {
                self.0.fetch_add(1, Ordering::SeqCst);
            }
        }

        let counter = Arc::new(AtomicUsize::new(0));
        let mut core = TerminalCore::new(80, 24, 0);
        core.callbacks = Some(Box::new(Recorder(counter.clone())));
        core.reset();
        assert_eq!(counter.load(Ordering::SeqCst), 1);
    }

    // ── Batch row packed ─────────────────────────────────

    #[test]
    fn test_get_row_packed_basic() {
        let mut core = TerminalCore::new(3, 1, 0);
        core.set_cell(0, 0, "A", 1, 0, 0, 0, 0, 0, 0, 0, 0, 0);
        let packed = core.get_row_packed(0);
        assert!(!packed.is_empty());
        // First byte should be char_len=1, then 'A'
        assert_eq!(packed[0], 1); // char_len
        assert_eq!(packed[1], b'A'); // char data
    }

    // ── Overflow side table with shift ───────────────────

    #[test]
    fn test_overflow_remapped_on_shift_up() {
        let mut core = TerminalCore::new(10, 5, 0);
        let long = "👨‍👩‍👧‍👦";
        assert!(long.as_bytes().len() > 16);
        core.set_cell(0, 3, long, 2, 0, 0, 0, 0, 0, 0, 0, 0, 0);
        assert_eq!(core.get_cell_char(0, 3), long);

        core.shift_rows_up(0, 4, 2);
        // Row 3 shifted to row 1
        assert_eq!(core.get_cell_char(0, 1), long);
    }

    // ── Phase 4: Reverse index tests ────────────────────

    #[test]
    fn test_ridx_maintained_on_set_cell_overflow() {
        let mut core = TerminalCore::new(10, 5, 0);
        let long = "👨‍👩‍👧‍👦";
        core.set_cell(3, 2, long, 2, 0, 0, 0, 0, 0, 0, 0, 0, 0);
        let abs = core.viewport_abs(2) as u32;
        assert!(core.overflow_ridx.contains_key(&abs));
        assert!(core.overflow_ridx[&abs].contains(&3));
    }

    #[test]
    fn test_ridx_removed_on_overwrite_with_ascii() {
        let mut core = TerminalCore::new(10, 5, 0);
        let long = "👨‍👩‍👧‍👦";
        core.set_cell(3, 2, long, 2, 0, 0, 0, 0, 0, 0, 0, 0, 0);
        let abs = core.viewport_abs(2) as u32;
        assert!(core.overflow_ridx.contains_key(&abs));

        // Overwrite with ASCII
        core.set_cell_ascii(3, 2, b'X', 0, 0, 0, 0, 0, 0, 0, 0, 0);
        assert!(!core.overflow_ridx.contains_key(&abs));
    }

    #[test]
    fn test_ridx_maintained_after_shift_rows_up() {
        let mut core = TerminalCore::new(10, 5, 0);
        let long = "👨‍👩‍👧‍👦";
        core.set_cell(5, 3, long, 2, 0, 0, 0, 0, 0, 0, 0, 0, 0);
        let old_abs = core.viewport_abs(3) as u32;
        assert!(core.overflow_ridx.contains_key(&old_abs));

        core.shift_rows_up(0, 4, 2);
        // Row 3 -> row 1
        let new_abs = core.viewport_abs(1) as u32;
        assert!(core.overflow_ridx.contains_key(&new_abs));
        assert!(core.overflow_ridx[&new_abs].contains(&5));
        // Old abs should be gone
        assert!(!core.overflow_ridx.contains_key(&old_abs));
    }

    #[test]
    fn test_ridx_maintained_after_shift_rows_down() {
        let mut core = TerminalCore::new(10, 5, 0);
        let long = "👨‍👩‍👧‍👦";
        core.set_cell(5, 1, long, 2, 0, 0, 0, 0, 0, 0, 0, 0, 0);
        let old_abs = core.viewport_abs(1) as u32;
        assert!(core.overflow_ridx.contains_key(&old_abs));

        core.shift_rows_down(0, 4, 2);
        // Row 1 -> row 3
        let new_abs = core.viewport_abs(3) as u32;
        assert!(core.overflow_ridx.contains_key(&new_abs));
        assert!(core.overflow_ridx[&new_abs].contains(&5));
    }

    #[test]
    fn test_ridx_cleared_on_clear_line() {
        let mut core = TerminalCore::new(10, 5, 0);
        let long = "👨‍👩‍👧‍👦";
        core.set_cell(5, 2, long, 2, 0, 0, 0, 0, 0, 0, 0, 0, 0);
        let abs = core.viewport_abs(2) as u32;
        assert!(core.overflow_ridx.contains_key(&abs));

        core.clear_line(2);
        assert!(!core.overflow_ridx.contains_key(&abs));
    }

    #[test]
    fn test_ridx_copy_row() {
        let mut core = TerminalCore::new(10, 5, 0);
        let long = "👨‍👩‍👧‍👦";
        core.set_cell(5, 1, long, 2, 0, 0, 0, 0, 0, 0, 0, 0, 0);

        core.copy_row(1, 3);
        let dst_abs = core.viewport_abs(3) as u32;
        assert!(core.overflow_ridx.contains_key(&dst_abs));
        assert!(core.overflow_ridx[&dst_abs].contains(&5));
        // Source should still have it
        let src_abs = core.viewport_abs(1) as u32;
        assert!(core.overflow_ridx.contains_key(&src_abs));
    }

    #[test]
    fn test_ridx_cleared_on_reset() {
        let mut core = TerminalCore::new(10, 5, 0);
        let long = "👨‍👩‍👧‍👦";
        core.set_cell(5, 2, long, 2, 0, 0, 0, 0, 0, 0, 0, 0, 0);
        assert!(!core.overflow_ridx.is_empty());

        core.reset();
        assert!(core.overflow_ridx.is_empty());
    }

    // ── process_pty_data interruptible tests ─────────────

    #[test]
    fn test_process_pty_data_normal_consumes_all() {
        let mut core = TerminalCore::new(80, 24, 0);
        let data = b"Hello";
        let consumed = core.process_pty_data(data);
        assert_eq!(consumed, data.len());
        assert!(core.mode_actions.is_empty());
    }

    #[test]
    fn test_reset_and_replay_paints_only_replay_bytes() {
        let mut core = TerminalCore::new(80, 24, 100);
        core.process_pty_data(b"old data");
        // Sanity: first cell now holds 'o'.
        assert_eq!(core.get_cell_char(0, 0), "o");
        // Replay a different stream.
        core.reset_and_replay(b"NEW");
        assert_eq!(core.get_cell_char(0, 0), "N");
        assert_eq!(core.get_cell_char(1, 0), "E");
        assert_eq!(core.get_cell_char(2, 0), "W");
        // Cell 3 must be empty after reset (no leftover from "old data").
        assert_eq!(core.get_cell_char(3, 0), " ");
    }

    #[test]
    fn test_reset_and_replay_empty_bytes_clears_grid() {
        let mut core = TerminalCore::new(80, 24, 100);
        core.process_pty_data(b"junk");
        core.reset_and_replay(b"");
        assert_eq!(core.get_cell_char(0, 0), " ");
    }

    #[test]
    fn test_process_pty_data_stops_on_buffer_switch() {
        let mut core = TerminalCore::new(80, 24, 0);
        // CSI ?1049h (8 bytes) followed by "AB"
        let data = b"\x1B[?1049hAB";
        let consumed = core.process_pty_data(data);
        assert_eq!(consumed, 8);
        assert!(core.has_pending_buffer_switch());
        // The mode action should be MODE_ACTION_SAVE_AND_SWITCH_TO_ALT (2)
        let actions = core.take_mode_actions();
        assert!(actions.contains(&2));
    }

    #[test]
    fn test_has_pending_buffer_switch_empty() {
        let core = TerminalCore::new(80, 24, 0);
        assert!(!core.has_pending_buffer_switch());
    }

    #[test]
    fn test_has_pending_buffer_switch_skips_ts_fallback() {
        let mut core = TerminalCore::new(80, 24, 0);
        // Simulate TS_FALLBACK entry: [0xFF, lo, hi]
        core.mode_actions.push(0xFF);
        core.mode_actions.push(0x01);
        core.mode_actions.push(0x00);
        assert!(!core.has_pending_buffer_switch());
    }

    #[test]
    fn test_has_pending_buffer_switch_detects_switch_to_alt() {
        let mut core = TerminalCore::new(80, 24, 0);
        core.mode_actions.push(1); // SWITCH_TO_ALT
        assert!(core.has_pending_buffer_switch());
    }

    #[test]
    fn test_has_pending_buffer_switch_detects_switch_to_main() {
        let mut core = TerminalCore::new(80, 24, 0);
        core.mode_actions.push(3); // SWITCH_TO_MAIN
        assert!(core.has_pending_buffer_switch());
    }

    // ── SGR combined RGB through full parse pipeline ──────

    #[test]
    fn test_process_pty_data_sgr_combined_rgb_fg_bg() {
        // Full pipeline test: raw bytes → parser → CSI dispatch → SGR handler.
        // ESC[38;2;200;200;200;48;2;43;48;59m = 10 SGR params
        // Then print 'X' to commit cursor attrs to a cell.
        let mut core = TerminalCore::new(80, 24, 0);
        let data = b"\x1b[38;2;200;200;200;48;2;43;48;59mX";
        let consumed = core.process_pty_data(data);
        assert_eq!(consumed, data.len());
        // Cell at (0,0) should have the correct colors
        let fg = PackedColor::from_u32(core.get_cell_fg(0, 0));
        let bg = PackedColor::from_u32(core.get_cell_bg(0, 0));
        assert_eq!(fg, PackedColor::rgb(200, 200, 200));
        assert_eq!(
            bg,
            PackedColor::rgb(43, 48, 59),
            "bg should be rgb(43,48,59), not indexed(3)"
        );
    }

    // ── Grapheme buffer flush on non-Print dispatch ──────

    #[test]
    fn test_grapheme_buffer_flushed_before_csi_cursor_move() {
        let mut core = TerminalCore::new(80, 24, 0);
        // Print emoji (gets buffered as Extended_Pictographic)
        // then CSI CUP to move cursor, then print 'A'
        // Emoji should be at position (0,0), not at the CUP destination
        let data = b"\xF0\x9F\x98\x80\x1B[3;5HA"; // 😀 \x1b[3;5H A
        core.process_pty_data(data);
        // 😀 should be at (0, 0)
        assert_eq!(core.get_cell_char(0, 0), "😀");
        assert_eq!(core.get_cell_width(0, 0), 2);
        // 'A' should be at (4, 2) [CUP row=3 col=5 → 0-indexed (2, 4)]
        assert_eq!(core.get_cell_char(4, 2), "A");
    }

    #[test]
    fn test_grapheme_buffer_flushed_before_execute_cr() {
        let mut core = TerminalCore::new(80, 24, 0);
        // Move cursor to col 10 first, print emoji then CR
        // Emoji should be at col 10 (flushed before CR), not lost
        let data = b"\x1B[1;11H\xF0\x9F\x98\x80\r"; // CUP(1,11) 😀 CR
        core.process_pty_data(data);
        // 😀 should be at (10, 0) with width 2
        assert_eq!(core.get_cell_char(10, 0), "😀");
        assert_eq!(core.get_cell_width(10, 0), 2);
        // After CR, cursor should be at col 0
        assert_eq!(core.get_cursor_col(), 0);
    }

    #[test]
    fn test_grapheme_buffer_flushed_before_execute_lf() {
        let mut core = TerminalCore::new(80, 24, 0);
        // Print emoji then LF then 'A'
        let data = b"\xF0\x9F\x98\x80\nA"; // 😀 LF A
        core.process_pty_data(data);
        // 😀 should be at (0, 0), width 2
        assert_eq!(core.get_cell_char(0, 0), "😀");
        assert_eq!(core.get_cell_width(0, 0), 2);
        // After LF, cursor moves to row 1 (col stays at 2 from emoji advance)
        // 'A' should be at (2, 1)
        assert_eq!(core.get_cell_char(2, 1), "A");
    }

    #[test]
    fn test_grapheme_buffer_flushed_before_esc_dispatch() {
        let mut core = TerminalCore::new(80, 24, 0);
        // Move to row 1 first so ESC M (Reverse Index) goes to row 0
        // Print emoji at row 1, then ESC M
        let data = b"\x1B[2;1H\xF0\x9F\x98\x80\x1BM"; // CUP(2,1) 😀 ESC_M
        core.process_pty_data(data);
        // 😀 should be at (0, 1) — row 1, col 0
        assert_eq!(core.get_cell_char(0, 1), "😀");
        assert_eq!(core.get_cell_width(0, 1), 2);
        // After ESC M (reverse index), cursor should be at row 0
        assert_eq!(core.get_cursor_row(), 0);
    }

    // ── DEC mode 1048 immediate save/restore ──────────────

    #[test]
    fn test_dec_1048_save_restore_immediate_in_data_stream() {
        let mut core = TerminalCore::new(80, 24, 0);
        // Write "AB" at (0,0), save cursor (CSI ?1048h), move to (10,5),
        // write "CD", restore cursor (CSI ?1048l), write "EF"
        // "EF" should appear at (2,0) (where cursor was saved), not at (12,5)
        let data = b"AB\x1B[?1048h\x1B[6;11HCD\x1B[?1048lEF";
        core.process_pty_data(data);
        // "AB" at (0,0) and (1,0)
        assert_eq!(core.get_cell_char(0, 0), "A");
        assert_eq!(core.get_cell_char(1, 0), "B");
        // "CD" at (10,5) and (11,5)
        assert_eq!(core.get_cell_char(10, 5), "C");
        assert_eq!(core.get_cell_char(11, 5), "D");
        // "EF" at (2,0) and (3,0) (restored cursor position)
        assert_eq!(core.get_cell_char(2, 0), "E");
        assert_eq!(core.get_cell_char(3, 0), "F");
        // No mode actions should be queued (handled immediately)
        assert!(core.mode_actions.is_empty());
    }

    #[test]
    fn test_dec_1048_and_esc7_share_same_saved_cursor() {
        let mut core = TerminalCore::new(80, 24, 0);
        // Save with ESC 7 at (5,3), move, restore with CSI ?1048l
        // They should share the same saved cursor slot
        core.set_cursor(5, 3);
        let data = b"\x1B7\x1B[10;20HX\x1B[?1048l";
        core.process_pty_data(data);
        // Cursor should be restored to (5,3) from ESC 7 save
        assert_eq!(core.get_cursor_col(), 5);
        assert_eq!(core.get_cursor_row(), 3);
    }

    // ── Cell size propagation tests ──────────────────────

    #[test]
    fn test_cell_size_defaults() {
        let core = TerminalCore::new(80, 24, 0);
        assert_eq!(core.get_cell_width_px(), 8);
        assert_eq!(core.get_cell_height_px(), 16);
    }

    #[test]
    fn test_cell_size_preserved_after_reset() {
        let mut core = TerminalCore::new(80, 24, 0);
        core.set_cell_size_px(10, 20);
        core.reset();
        // Cell size is not reset (app-managed, not terminal state)
        assert_eq!(core.get_cell_width_px(), 10);
        assert_eq!(core.get_cell_height_px(), 20);
    }

    #[test]
    fn test_xtwinops_cell_size_after_buffer_switch_defaults() {
        // Simulates the problem: a new alternate core starts with default 8x16
        // CSI 16t should return the default before cell size is set
        let mut core = TerminalCore::new(80, 24, 0);
        let len = core.handle_xtwinops_cell_size();
        assert!(len > 0);
        let bytes = core.get_response_bytes();
        assert_eq!(&bytes, b"\x1b[6;16;8t");

        // After setting cell size, CSI 16t should return the new values
        core.set_cell_size_px(10, 20);
        core.handle_xtwinops_cell_size();
        let bytes = core.get_response_bytes();
        assert_eq!(&bytes, b"\x1b[6;20;10t");
    }

    // ── BCE (Background Color Erase) tests ──────────────

    /// Helper: set cursor bg to green (indexed color 2)
    fn set_cursor_bg_green(core: &mut TerminalCore) {
        core.set_cursor_bg(1, 2, 0, 0); // tag=1 (indexed), index=2 (green)
    }

    #[test]
    fn test_bce_clear_line() {
        let mut core = TerminalCore::new(10, 3, 0);
        set_cursor_bg_green(&mut core);
        core.clear_line(0);
        for col in 0..10 {
            let bg = PackedColor::from_u32(core.get_cell_bg(col, 0));
            assert_eq!(
                bg,
                PackedColor::indexed(2),
                "col {col} should have green bg"
            );
        }
    }

    #[test]
    fn test_bce_clear_line_range() {
        let mut core = TerminalCore::new(10, 3, 0);
        set_cursor_bg_green(&mut core);
        core.clear_line_range(0, 3, 7);
        for col in 3..7 {
            let bg = PackedColor::from_u32(core.get_cell_bg(col, 0));
            assert_eq!(
                bg,
                PackedColor::indexed(2),
                "col {col} should have green bg"
            );
        }
        // Cols outside range should still be default
        let bg0 = PackedColor::from_u32(core.get_cell_bg(0, 0));
        assert_eq!(bg0, PackedColor::DEFAULT);
        let bg9 = PackedColor::from_u32(core.get_cell_bg(9, 0));
        assert_eq!(bg9, PackedColor::DEFAULT);
    }

    #[test]
    fn test_bce_default_bg_unchanged() {
        // When cursor.bg is DEFAULT, erased cells should have DEFAULT bg
        let mut core = TerminalCore::new(10, 3, 0);
        // cursor.bg is already DEFAULT
        core.clear_line(0);
        for col in 0..10 {
            let bg = PackedColor::from_u32(core.get_cell_bg(col, 0));
            assert_eq!(bg, PackedColor::DEFAULT);
        }
    }

    #[test]
    fn test_bce_sgr_reset_then_erase() {
        let mut core = TerminalCore::new(10, 3, 0);
        // Set green bg
        set_cursor_bg_green(&mut core);
        // Reset cursor attrs (simulates ESC[0m)
        core.reset_cursor_attrs();
        core.clear_line(0);
        for col in 0..10 {
            let bg = PackedColor::from_u32(core.get_cell_bg(col, 0));
            assert_eq!(
                bg,
                PackedColor::DEFAULT,
                "After reset, bg should be DEFAULT"
            );
        }
    }

    #[test]
    fn test_bce_256_color() {
        let mut core = TerminalCore::new(10, 3, 0);
        core.set_cursor_bg(1, 196, 0, 0); // indexed color 196
        core.clear_line(0);
        let bg = PackedColor::from_u32(core.get_cell_bg(0, 0));
        assert_eq!(bg, PackedColor::indexed(196));
    }

    #[test]
    fn test_bce_rgb_color() {
        let mut core = TerminalCore::new(10, 3, 0);
        core.set_cursor_bg(2, 100, 200, 50); // RGB
        core.clear_line(0);
        let bg = PackedColor::from_u32(core.get_cell_bg(0, 0));
        assert_eq!(bg, PackedColor::rgb(100, 200, 50));
    }

    #[test]
    fn test_bce_shift_rows_up() {
        let mut core = TerminalCore::new(10, 5, 0);
        for row in 0..5 {
            for col in 0..10 {
                core.set_cell_ascii(col, row, b'A', 0, 0, 0, 0, 0, 0, 0, 0, 0);
            }
        }
        set_cursor_bg_green(&mut core);
        core.shift_rows_up(0, 4, 2);
        // Vacated bottom rows (3, 4) should have green bg
        for row in 3..5 {
            for col in 0..10 {
                let bg = PackedColor::from_u32(core.get_cell_bg(col, row));
                assert_eq!(bg, PackedColor::indexed(2), "row {row} col {col}");
            }
        }
    }

    // ── SlimCell stats tests (FR11) ──────────────────────

    #[test]
    fn test_slim_cell_total_initial_zero() {
        let core = TerminalCore::new(80, 24, 100);
        assert_eq!(core.slim_cell_total(), 0);
    }

    #[test]
    fn test_slim_cell_total_after_eviction() {
        let mut core = TerminalCore::new(10, 3, 5);
        for r in 0..3 {
            core.set_cell(0, r, "X", 1, 0, 0, 0, 0, 0, 0, 0, 0, 0);
        }
        core.scroll_up_internal(2); // 2 rows go to scrollback
        // Each scrollback row has 10 SlimCells.
        assert_eq!(core.slim_cell_total(), 20);
    }

    // ── BCE shift_rows_down test ────────────────────────

    #[test]
    fn test_bce_shift_rows_down() {
        let mut core = TerminalCore::new(10, 5, 0);
        for row in 0..5 {
            for col in 0..10 {
                core.set_cell_ascii(col, row, b'A', 0, 0, 0, 0, 0, 0, 0, 0, 0);
            }
        }
        set_cursor_bg_green(&mut core);
        core.shift_rows_down(0, 4, 2);
        // Vacated top rows (0, 1) should have green bg
        for row in 0..2 {
            for col in 0..10 {
                let bg = PackedColor::from_u32(core.get_cell_bg(col, row));
                assert_eq!(bg, PackedColor::indexed(2), "row {row} col {col}");
            }
        }
    }

    // ── Reparse-cost measurement harness (FR1) ───────────
    //
    // A deterministic, on-demand timing harness that feeds a synthetic
    // scrollback through `process_pty_data_fully` on a fresh core and reports
    // the elapsed time + throughput. The synthetic input is fixed (no RNG, no
    // clock) so re-runs are stable, and the harness calls `term_core` directly
    // (no `App::pump_all`, no real PTY) so it is isolated from the flaky pump
    // path. The timing test is `#[ignore]`-gated so the default `cargo test`
    // run is unaffected; run it with `cargo test -- --ignored --nocapture`.

    /// Build a deterministic, terminal-representative byte buffer of about
    /// `target_bytes` bytes. The content mixes printable ASCII text, periodic
    /// newlines (so the parser scrolls and fills scrollback), and an occasional
    /// SGR colour change — no RNG and no clock input, so the buffer is
    /// byte-for-byte reproducible across runs and machines.
    fn build_synthetic_scrollback(target_bytes: usize) -> Vec<u8> {
        // A short, fixed palette of SGR colour changes cycled deterministically.
        const SGRS: &[&[u8]] = &[
            b"\x1b[31m", // red
            b"\x1b[32m", // green
            b"\x1b[33m", // yellow
            b"\x1b[34m", // blue
            b"\x1b[0m",  // reset
        ];
        // Printable glyphs cycled per column (deterministic, ASCII only).
        const GLYPHS: &[u8] = b"abcdefghijklmnopqrstuvwxyz0123456789 ";

        let mut out: Vec<u8> = Vec::with_capacity(target_bytes + 64);
        let mut col: usize = 0;
        let mut line: usize = 0;
        let mut glyph_i: usize = 0;
        // Wrap-ish line width so newlines come periodically (~80 cols).
        const LINE_WIDTH: usize = 78;

        while out.len() < target_bytes {
            if col == 0 {
                // Once every 8 lines, emit a deterministic SGR change so the
                // stream exercises the colour path without dominating it.
                if line % 8 == 0 {
                    out.extend_from_slice(SGRS[(line / 8) % SGRS.len()]);
                }
            }
            out.push(GLYPHS[glyph_i % GLYPHS.len()]);
            glyph_i += 1;
            col += 1;
            if col >= LINE_WIDTH {
                out.push(b'\r');
                out.push(b'\n');
                col = 0;
                line += 1;
            }
        }
        out
    }

    #[test]
    fn test_synthetic_scrollback_is_deterministic() {
        // Same size in -> byte-identical buffer out (no RNG / clock).
        let a = build_synthetic_scrollback(64 * 1024);
        let b = build_synthetic_scrollback(64 * 1024);
        assert_eq!(a, b, "synthetic scrollback must be reproducible");
        assert!(
            a.len() >= 64 * 1024,
            "buffer should reach the requested size"
        );
        // Sanity: it contains newlines and at least one SGR introducer.
        assert!(a.contains(&b'\n'), "should contain newlines");
        assert!(
            a.windows(2).any(|w| w == b"\x1b["),
            "should contain SGR sequences"
        );
    }

    #[test]
    fn test_reparse_empty_input_no_panic() {
        // FR1 empty-input guard: feeding 0 bytes through the full-drain reparse
        // path neither panics nor misreports; elapsed time is ~0 ms.
        let mut core = TerminalCore::new(80, 24, 10_000);
        let start = std::time::Instant::now();
        let actions = core.process_pty_data_fully(b"");
        let elapsed = start.elapsed();
        assert!(actions.is_empty(), "empty input yields no mode actions");
        // ~0 ms: be generous to avoid flakiness on loaded CI, but it must not
        // wander into the tens of ms a real reparse would take.
        assert!(
            elapsed.as_millis() < 50,
            "empty reparse should be ~0 ms, was {:?}",
            elapsed
        );
    }

    /// Gated measurement harness (FR1 -> FR2). Excluded from the default
    /// `cargo test` run via `#[ignore]`. Run explicitly with:
    ///
    /// ```text
    /// cargo test -p term_core -- --ignored --nocapture
    /// ```
    ///
    /// Reports the reparse time + throughput for a ~2 MiB synthetic scrollback,
    /// plus a few smaller sizes to show scaling. The measured ~2 MiB figure is
    /// the input to the §4 threshold decision recorded at verify time.
    #[test]
    #[ignore = "measurement harness; run with --ignored --nocapture"]
    fn measure_reparse_cost_2mib() {
        // Sizes: 256 KiB / 1 MiB / 2 MiB so scaling is visible. The 2 MiB run
        // is the headline figure for the go/no-go decision.
        const SIZES: &[(usize, &str)] = &[
            (256 * 1024, "256 KiB"),
            (1024 * 1024, "1 MiB"),
            (2 * 1024 * 1024, "2 MiB"),
        ];

        eprintln!("=== reparse-cost measurement (process_pty_data_fully) ===");
        for &(size, label) in SIZES {
            let buf = build_synthetic_scrollback(size);
            // Fresh core at a representative grid size with a 2 MiB-ish
            // scrollback capacity so rows actually accumulate.
            let mut core = TerminalCore::new(80, 24, 50_000);

            let start = std::time::Instant::now();
            let _ = core.process_pty_data_fully(&buf);
            let elapsed = start.elapsed();

            let bytes = buf.len() as f64;
            let secs = elapsed.as_secs_f64();
            let mib = bytes / (1024.0 * 1024.0);
            let mibps = if secs > 0.0 {
                mib / secs
            } else {
                f64::INFINITY
            };
            eprintln!(
                "{label:>8}: {bytes:>9.0} bytes  {ms:>8.3} ms  {mibps:>8.1} MiB/s",
                bytes = bytes,
                ms = elapsed.as_secs_f64() * 1000.0,
                mibps = mibps,
            );
        }
        eprintln!("=========================================================");
    }
}
