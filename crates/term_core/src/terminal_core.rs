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
    /// Side-table populated only during a snapshot-replay bypass
    /// (`scrollback_bypass == true`). When an OSC 133 B (CommandStart) mark
    /// is emitted while the bypass is on, the plain text of the cursor row
    /// at that instant is captured here under `abs_row → text`. This is
    /// necessary because the bypass intentionally discards scrollback
    /// contents: once the row scrolls past the viewport into the virtual
    /// scrollback it is irrecoverable. The downstream consumer
    /// (`tabs.rs::extract_line_text`) prefers this pre-captured text over a
    /// scrollback lookup when processing the drained `SnapshotReplay`.
    /// Drained by `take_bypass_b_mark_texts` (called from
    /// `build_from_snapshot` and shipped on `SnapshotReplay`). Cleared by
    /// `reset()`. Only populated while the bypass is active; remains empty
    /// on the normal live-PTY path.
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

    /// Drain any rows `resize_reflow` populated into `scrollback_slim` while
    /// `scrollback_bypass` was on, folding their count into the SAME virtual
    /// bookkeeping (`virtual_scrollback_len` / `scrollback_evicted_total`)
    /// [`Self::enable_snapshot_bypass`] documents — i.e. treat them exactly
    /// as `ring_push_blank`'s bypass branch would have, had it been the one
    /// to evict them, so bookkeeping stays byte-identical instead of merely
    /// "close". See [`Self::resize`]'s call site for why this must run.
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
    /// Honors in-band resize markers (IMPLEMENTATION.md D1, task0001) via
    /// [`Self::replay_with_resize_markers`] — see that method for the fix
    /// this implements (the resize-interleaved scrollback replay coordinate
    /// drift, `tmp/apt-progress-bar-regression-2026-07-09.md` PROBE D).
    pub fn reset_and_replay(&mut self, bytes: &[u8]) -> Vec<u8> {
        self.reset();
        // The non-cancellable entry point: delegates to the cancellable
        // drain with a flag that is never set, so `reset_and_replay` and
        // `build_from_snapshot`'s cancellable drain share one
        // marker-interpretation implementation and cannot drift. `NEVER` is
        // never stored to, so the drain always runs to completion and
        // returns `Some` — the unwrap cannot fail.
        static NEVER: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
        self.replay_with_resize_markers(bytes, &NEVER)
            .expect("non-cancellable drain always completes")
    }

    /// Replay `bytes` into `self`, honoring any in-band resize markers
    /// (IMPLEMENTATION.md D1, task0001) recorded by
    /// `mux::scrollback_buffer::resize_marker_bytes` at each daemon-side
    /// pane resize (`MuxPane::resize`): the stream is split at each marker,
    /// and `self` is resized to the marker's dimensions before the bytes
    /// recorded under THAT size are fed to the parser.
    ///
    /// This is the fix for the resize-interleaved scrollback replay
    /// coordinate drift (`tmp/apt-progress-bar-regression-2026-07-09.md`
    /// PROBE D): a replay core fixed at one row count misinterprets
    /// DECSTBM / CUP coordinates recorded for a DIFFERENT row count, mixing
    /// content from two logical output lines onto one row. Feeding each
    /// segment under the row count it was produced for keeps the record and
    /// replay coordinate systems in agreement throughout.
    ///
    /// After the final segment, `self` is resized back to its dimensions at
    /// the START of this call (the caller's requested / current pane size)
    /// if a marker changed them, so a replay with any number of intervening
    /// resizes always ends at the size the caller asked for.
    ///
    /// A marker-free `bytes` never matches [`find_resize_marker`], so this
    /// reduces to a single unsplit `process_pty_data_fully_cancellable`
    /// call — byte-for-byte identical to the pre-marker-support replay
    /// (task0001 AC-3).
    ///
    /// `cancel` is threaded straight through to each segment's
    /// `process_pty_data_fully_cancellable`; a flag observed mid-drain
    /// aborts the whole replay and returns `None`, matching that function's
    /// contract (a superseded off-thread `build_from_snapshot` worker bails
    /// out at the next chunk boundary instead of finishing the parse).
    ///
    /// **Reflow coalescing (review round-1 rework, finding
    /// `6ff208bbc674189c`, high):** a resize is DEFERRED — tracked in
    /// `pending` — rather than applied the instant its marker is found. It
    /// is only actually applied (via [`Self::resize`], the content-preserving
    /// full reflow) immediately before the next NON-EMPTY byte segment is
    /// fed, or at the very end if the final segment is non-empty. A run of
    /// consecutive markers with no bytes between them (e.g. a window drag
    /// that stamps a marker per intermediate size before output resumes at
    /// the final size) therefore costs at most ONE reflow — for the LAST
    /// pending size — instead of one full-scrollback reflow per marker. If
    /// no non-empty segment ever follows a pending resize (the recording
    /// ends right after a marker run), zero reflows happen for it: nothing
    /// was ever fed at any of those intermediate sizes, so there is nothing
    /// to preserve content FOR. This also naturally satisfies "avoid the
    /// final restore reflow when the pending dimensions already match the
    /// target": the final `(self.cols, self.rows) != (target_cols,
    /// target_rows)` check runs against whatever size was last actually
    /// applied, so it is skipped whenever that already equals the target
    /// (including the zero-reflow case above, where `self`'s size never
    /// changed from the target at all).
    fn replay_with_resize_markers(
        &mut self,
        bytes: &[u8],
        cancel: &std::sync::atomic::AtomicBool,
    ) -> Option<Vec<u8>> {
        let target_cols = self.cols;
        let target_rows = self.rows;
        let mut actions = Vec::new();
        let mut offset = 0usize;
        // Deferred resize target: the dimensions the NEXT non-empty segment
        // must be fed at. `None` means no resize is currently pending.
        let mut pending: Option<(u16, u16)> = None;
        while let Some((marker_start, marker_end, cols, rows)) = find_resize_marker(bytes, offset) {
            let segment = &bytes[offset..marker_start];
            if !segment.is_empty() {
                if let Some((pending_cols, pending_rows)) = pending.take() {
                    if (self.cols, self.rows) != (pending_cols, pending_rows) {
                        self.resize(pending_cols, pending_rows);
                    }
                }
                actions.extend(self.process_pty_data_fully_cancellable(segment, cancel)?);
            }
            pending = Some((cols, rows));
            offset = marker_end;
        }
        let tail = &bytes[offset..];
        if !tail.is_empty() {
            if let Some((pending_cols, pending_rows)) = pending.take() {
                if (self.cols, self.rows) != (pending_cols, pending_rows) {
                    self.resize(pending_cols, pending_rows);
                }
            }
            actions.extend(self.process_pty_data_fully_cancellable(tail, cancel)?);
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
        cancel: &std::sync::atomic::AtomicBool,
    ) -> Option<SnapshotReplay> {
        Self::build_from_snapshot_inner(cols, rows, scrollback_lines, payload, cancel, true)
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
        cancel: &std::sync::atomic::AtomicBool,
    ) -> Option<SnapshotReplay> {
        Self::build_from_snapshot_inner(cols, rows, scrollback_lines, payload, cancel, false)
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
        cancel: &std::sync::atomic::AtomicBool,
        bypass: bool,
    ) -> Option<SnapshotReplay> {
        // D6 (task0003, review round-2 finding `893241823258fce3`): a
        // row-count-GROWING mid-drain resize needs to pull rows up from
        // scrollback to populate the newly visible rows — but the bypass
        // keeps `scrollback_slim` deliberately empty during the drain (see
        // below), so those rows come up blank and the cursor's row
        // recalculation drifts by `(new_rows - old_rows)`, diverging from
        // what the synchronous (non-bypass) path produces for the SAME
        // payload. Rather than a fragile mid-stream bypass toggle (which
        // would violate this function's "scrollback_slim stays empty
        // throughout" contract for its caller, `build_from_snapshot`),
        // downgrade to the non-bypass recipe for the WHOLE replay whenever
        // the payload contains any row-growing marker, so growth always
        // pulls real content exactly as the synchronous path does.
        let bypass = bypass && !payload_has_row_growing_marker(rows, payload);
        let mut core = TerminalCore::new(cols, rows, scrollback_lines);
        core.reset();
        // Snapshot-replay bypass: skip per-row SlimCell compression during the
        // drain (the dominant cost on a heavy `seq`-shaped payload). The
        // bypass keeps `evicted_total` + mark stamping byte-identical to
        // today's path via `virtual_scrollback_len`; only the post-replay
        // scrollback *contents* are intentionally not populated. The 2nd-pass
        // scrollback-restore worker needs the contents, so it sets
        // `bypass = false` and pays the per-row compression cost.
        //
        // task0001 narrow interaction, closed by review round-1 rework
        // (finding `1698d9b52a89e241`): if `payload` contains a resize
        // marker, `replay_with_resize_markers` below calls the
        // content-preserving `resize` (full reflow) mid-drain, which is NOT
        // itself bypass-aware and can populate `scrollback_slim` even while
        // `bypass == true`. `TerminalCore::resize` now restores the bypass
        // invariant (drains any such leaked rows back out, folding their
        // count into the SAME virtual bookkeeping `ring_push_blank`'s
        // bypass branch would have used) on every call while
        // `scrollback_bypass` is on, so this no longer corrupts EITHER the
        // returned grid/cursor OR `evicted_total` / `get_scrollback_length()`
        // — and, more importantly, no longer leaves residual rows for the
        // 2nd-pass `merge_scrollback_from` to mistake for genuine post-swap
        // live-drain content. The debug_assert right before
        // `disable_snapshot_bypass` below is the regression guard: it fails
        // loudly in tests if a FUTURE in-drain mutation path reintroduces a
        // leak this fix doesn't already cover.
        if bypass {
            core.enable_snapshot_bypass();
        }
        let actions = match core.replay_with_resize_markers(payload, cancel) {
            Some(a) => a,
            None => {
                // Cancelled mid-drain: leave the core consistent (clear the
                // bypass) before discarding it via the `None` return so a
                // debugger / panic handler that touches the dropped core
                // doesn't see a half-set bypass.
                if bypass {
                    core.disable_snapshot_bypass();
                }
                return None;
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
        if bypass {
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
        Some(SnapshotReplay {
            core,
            actions,
            evicted_total,
            prompt_marks,
            fold_marks,
            bypass_b_mark_texts,
        })
    }

    /// Enable the snapshot-replay bypass: subsequent `ring_push_blank`
    /// evictions skip the SlimCell intern + `scrollback_slim` push/pop work
    /// (the per-row hot loop), but still bump `virtual_scrollback_len` /
    /// `scrollback_evicted_total` so the observable bookkeeping is byte-
    /// identical to the live path on the same payload.
    ///
    /// Preconditions (asserted): the scrollback deque is empty and the
    /// virtual length is zero. These hold immediately after `reset()` and
    /// on a freshly-constructed core, which is the only place the bypass is
    /// turned on (inside `build_from_snapshot`).
    pub(crate) fn enable_snapshot_bypass(&mut self) {
        assert!(
            self.scrollback_slim.is_empty() && self.virtual_scrollback_len == 0,
            "enable_snapshot_bypass requires an empty scrollback deque and a zero virtual length"
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
        // Under the snapshot-replay bypass, the scrollback contents are
        // intentionally discarded, so once this row scrolls off the viewport
        // its text is irrecoverable from the bypassed store. Capture the
        // cursor row's plain text NOW, at B-mark emission time, so the
        // downstream consumer can use it instead of a scrollback lookup.
        // Only B (CommandStart) carries command text; A/C/D do not need this.
        if self.scrollback_bypass && kind == b'B' {
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

// ── Resize-marker replay interpretation (IMPLEMENTATION.md D1/D2, task0001) ─

/// Prefix of the in-band resize marker: `ESC ] 777 ; emterm ; resize ;
/// <cols> ; <rows> BEL`.
///
/// task0003 D2 (review round-2 finding `602e685494248cbb`): this byte format
/// used to be independently duplicated — the encoder lived in the `emterm`
/// daemon crate's `mux::scrollback_buffer::resize_marker_bytes` with its own
/// literal prefix, while the decoder here held a SEPARATE literal and its
/// own dimension bounds, so the two could drift (the daemon accepted any
/// `u16` while this module rejected anything above 2000, silently dropping
/// legitimate large resizes). `term_core` has no dependency on the daemon
/// crate, but the daemon crate DOES depend on `term_core` (it already uses
/// `TerminalCore` directly), so this module is now the single owner of both
/// the byte format and the accepted dimension range: [`resize_marker_bytes`]
/// (the encoder), [`find_resize_marker`] / [`parse_resize_marker_dims`] (the
/// decoder), and [`RESIZE_MARKER_MAX_COLS`] / [`RESIZE_MARKER_MAX_ROWS`] (the
/// shared bounds) are all `pub` so `mux::scrollback_buffer::resize_marker_bytes`
/// can become a thin re-export instead of an independent literal, and so the
/// daemon can validate a resize against the SAME bounds the decoder accepts
/// at the point the resize is applied (`MuxPane::resize`), rather than
/// silently losing a marker at replay time.
const RESIZE_MARKER_PREFIX: &[u8] = b"\x1b]777;emterm;resize;";

/// Encode an in-band resize marker: `ESC ] 777 ; emterm ; resize ; <cols> ;
/// <rows> BEL`. SSOT for the marker byte format (task0003 D2) — see
/// [`RESIZE_MARKER_PREFIX`]'s doc comment. `mux::scrollback_buffer::resize_marker_bytes`
/// (in the `emterm` daemon crate) re-exports this rather than holding an
/// independent literal.
///
/// Rides the same envelope as the other `emterm` OSC 777 extensions (fold /
/// status-bar / agent-status / viewer launches): it is inert to a
/// marker-unaware replay consumer, since an unrecognized, BEL-terminated OSC
/// is parsed structurally and dropped without producing a visible cell.
pub fn resize_marker_bytes(cols: u16, rows: u16) -> Vec<u8> {
    format!("\x1b]777;emterm;resize;{cols};{rows}\x07").into_bytes()
}

/// Maximum length in bytes of a well-formed marker body (`<cols>;<rows>`),
/// e.g. `"65535;65535"`. A BEL found beyond this many bytes past a
/// candidate's prefix cannot terminate a valid marker body by definition,
/// so the BEL scan is bounded to this window rather than running to the
/// end of `bytes` (see `find_resize_marker`).
const RESIZE_MARKER_MAX_BODY_LEN: usize = 11;

/// Scan `bytes[from..]` for the next complete, well-formed resize marker.
/// Returns `(marker_start, marker_end, cols, rows)`: `marker_start..marker_end`
/// is the marker's full byte span (introducer through the terminating BEL,
/// inclusive) to excise from the replay stream, and `(cols, rows)` are its
/// parsed dimensions.
///
/// Returns `None` when no marker is found. A candidate that starts with the
/// marker prefix but is truncated (no BEL before the end of `bytes`) or
/// malformed (non-numeric / zero fields / extra fields) is treated as "not a
/// marker" and left for the normal ANSI parser to consume as an ordinary
/// (harmless, unrecognized) OSC sequence — an unterminated candidate stops
/// the scan entirely (mirrors the daemon-side scrollback filters' "leave a
/// partial sequence alone" discipline), while a terminated-but-malformed one
/// only skips past its own prefix so a later, well-formed marker is still
/// found. A BEL found only outside the `RESIZE_MARKER_MAX_BODY_LEN` window
/// is likewise treated as "not our marker" (not a valid body length) and
/// only skips past the candidate's own prefix.
///
/// `pub` (task0003 D2 / D1): besides the replay decoder (this module), the
/// `emterm` daemon crate's write-path scrollback filter
/// (`mux::scrollback_filter::strip_pty_output_for_scrollback_write`) calls
/// this directly to scrub ANY literal occurrence of a marker-shaped byte
/// sequence from PTY-sourced content — including one nested inside a
/// sequence the daemon's ANSI-structural strip would otherwise preserve
/// whole (e.g. a non-SIXEL DCS body; review round-2 finding
/// `15c54fb74bb91ec7`) — using the EXACT same scan the decoder itself uses,
/// so the two can never disagree about what counts as a marker.
pub fn find_resize_marker(bytes: &[u8], from: usize) -> Option<(usize, usize, u16, u16)> {
    let mut search_from = from;
    while search_from + RESIZE_MARKER_PREFIX.len() <= bytes.len() {
        let rel = bytes[search_from..]
            .windows(RESIZE_MARKER_PREFIX.len())
            .position(|w| w == RESIZE_MARKER_PREFIX)?;
        let marker_start = search_from + rel;
        let body_start = marker_start + RESIZE_MARKER_PREFIX.len();
        let window_end = (body_start + RESIZE_MARKER_MAX_BODY_LEN + 1).min(bytes.len());
        let bel_rel = match bytes[body_start..window_end]
            .iter()
            .position(|&b| b == 0x07)
        {
            Some(rel) => rel,
            None if window_end == bytes.len() => return None,
            None => {
                // No BEL within the valid-body-length window, but the
                // stream continues past it: this candidate is not our
                // marker. Resume scanning just past this candidate's
                // introducer so an overlapping later match is still found.
                search_from = marker_start + 1;
                continue;
            }
        };
        let bel_at = body_start + bel_rel;
        if let Some((cols, rows)) = parse_resize_marker_dims(&bytes[body_start..bel_at]) {
            return Some((marker_start, bel_at + 1, cols, rows));
        }
        // Terminated but malformed body: not actually our marker. Resume
        // scanning just past this candidate's introducer so an overlapping
        // later match is still found, without looping on the same failed
        // candidate.
        search_from = marker_start + 1;
    }
    None
}

/// Scan `payload` for any resize marker whose `rows` exceeds the row count
/// in effect immediately before it (starting from `initial_rows`, the
/// dimensions the replay core is constructed at) — task0003 D6, used by
/// [`TerminalCore::build_from_snapshot_inner`] to decide whether the
/// snapshot-replay bypass is safe for a given payload (see that function's
/// doc comment for why a row-count GROWTH during the bypass diverges from
/// the synchronous path).
fn payload_has_row_growing_marker(initial_rows: u16, payload: &[u8]) -> bool {
    let mut prev_rows = initial_rows;
    let mut offset = 0usize;
    while let Some((_, end, _, marker_rows)) = find_resize_marker(payload, offset) {
        if marker_rows > prev_rows {
            return true;
        }
        prev_rows = marker_rows;
        offset = end;
    }
    // `replay_with_resize_markers` always restores to `initial_rows` (the
    // construction / target size) at the end of the drain, even when no
    // marker explicitly asks for it. If the last marker (or the initial
    // size, when there are none) left FEWER rows than that, this implicit
    // final restore is itself a row-count growth and needs the same
    // treatment as an explicit one.
    initial_rows > prev_rows
}

/// Defensive upper bound on a resize marker's `cols` field. Marker
/// dimensions are replayed directly into `TerminalCore::resize`, which
/// allocates `(scrollback_capacity + rows) * cols` cells — an unbounded
/// value here would let a forged or corrupted marker trigger a huge
/// allocation. Comfortably above any real terminal width.
///
/// `pub` (task0003 D2, review round-2 finding `602e685494248cbb`): the
/// daemon's encoder previously accepted any `u16` while this decoder
/// rejected anything above the (private) old value of 2000, so a legitimate
/// resize past that value silently lost its marker at replay — the two
/// sides disagreed on the accepted range with nothing to catch the drift.
/// Exporting this constant lets the daemon validate/clamp a resize against
/// the SAME bound at the point it is applied (`MuxPane::resize`,
/// `term_core::clamp_resize_dims`), so anything that reaches the encoder is
/// guaranteed within what this decoder accepts.
pub const RESIZE_MARKER_MAX_COLS: u16 = 4096;

/// Defensive upper bound on a resize marker's `rows` field. See
/// [`RESIZE_MARKER_MAX_COLS`] for the rationale (both the `pub` visibility
/// and the shared-bound reasoning apply identically here).
pub const RESIZE_MARKER_MAX_ROWS: u16 = 4096;

/// Clamp a requested resize to the range [`resize_marker_bytes`] /
/// [`parse_resize_marker_dims`] agree on (`1..=RESIZE_MARKER_MAX_COLS` /
/// `1..=RESIZE_MARKER_MAX_ROWS`), task0003 D2. Called at the point a resize
/// is APPLIED (`MuxPane::resize`) so the dimensions that reach the marker
/// encoder — and the PTY itself — can never be silently un-representable by
/// the decoder later. `0` clamps up to `1` (a zero-sized terminal is not
/// meaningful) rather than down, matching the decoder's zero rejection.
pub fn clamp_resize_dims(cols: u16, rows: u16) -> (u16, u16) {
    (
        cols.clamp(1, RESIZE_MARKER_MAX_COLS),
        rows.clamp(1, RESIZE_MARKER_MAX_ROWS),
    )
}

/// Parse a resize marker body (`<cols>;<rows>`, no leading/trailing bytes).
/// Rejects a non-numeric, extra-field, zero-valued, or over-large dimension
/// (see [`RESIZE_MARKER_MAX_COLS`] / [`RESIZE_MARKER_MAX_ROWS`]).
fn parse_resize_marker_dims(body: &[u8]) -> Option<(u16, u16)> {
    let s = std::str::from_utf8(body).ok()?;
    let mut parts = s.split(';');
    let cols: u16 = parts.next()?.parse().ok()?;
    let rows: u16 = parts.next()?.parse().ok()?;
    if parts.next().is_some()
        || cols == 0
        || rows == 0
        || cols > RESIZE_MARKER_MAX_COLS
        || rows > RESIZE_MARKER_MAX_ROWS
    {
        return None;
    }
    Some((cols, rows))
}

// ── Tests ────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── Resize-marker scanning (task0001, IMPLEMENTATION.md D1/D2) ───────

    #[test]
    fn find_resize_marker_locates_well_formed_marker() {
        let bytes = b"before\x1b]777;emterm;resize;120;48\x07after";
        let found = find_resize_marker(bytes, 0).expect("marker must be found");
        let (start, end, cols, rows) = found;
        assert_eq!(&bytes[start..end], b"\x1b]777;emterm;resize;120;48\x07");
        assert_eq!(cols, 120);
        assert_eq!(rows, 48);
        assert_eq!(&bytes[..start], b"before");
        assert_eq!(&bytes[end..], b"after");
    }

    #[test]
    fn find_resize_marker_none_when_absent() {
        let bytes = b"plain text with no marker at all, not even an ESC";
        assert!(find_resize_marker(bytes, 0).is_none());
    }

    #[test]
    fn find_resize_marker_none_when_unterminated() {
        // Starts the marker prefix but never reaches a BEL terminator.
        let bytes = b"before\x1b]777;emterm;resize;120;48";
        assert!(find_resize_marker(bytes, 0).is_none());
    }

    #[test]
    fn find_resize_marker_skips_malformed_candidate_and_finds_next() {
        // First candidate has a non-numeric field (malformed); the second,
        // well-formed marker later in the stream must still be found.
        let mut bytes = b"\x1b]777;emterm;resize;abc;48\x07".to_vec();
        bytes.extend_from_slice(b"middle\x1b]777;emterm;resize;80;24\x07tail");
        let (start, end, cols, rows) = find_resize_marker(&bytes, 0).expect("must find the second");
        assert_eq!(cols, 80);
        assert_eq!(rows, 24);
        assert_eq!(&bytes[end..], b"tail");
        assert!(start > 0, "must have skipped past the malformed candidate");
    }

    #[test]
    fn find_resize_marker_rejects_zero_dimension() {
        let bytes = b"\x1b]777;emterm;resize;0;48\x07";
        assert!(find_resize_marker(bytes, 0).is_none());
        let bytes2 = b"\x1b]777;emterm;resize;120;0\x07";
        assert!(find_resize_marker(bytes2, 0).is_none());
    }

    #[test]
    fn find_resize_marker_rejects_extra_field() {
        let bytes = b"\x1b]777;emterm;resize;120;48;99\x07";
        assert!(find_resize_marker(bytes, 0).is_none());
    }

    #[test]
    fn find_resize_marker_search_from_offset_skips_earlier_marker() {
        let bytes = b"\x1b]777;emterm;resize;10;10\x07\x1b]777;emterm;resize;20;20\x07";
        // Search starting right after the first marker's introducer must
        // skip the first and find the second.
        let first_end = find_resize_marker(bytes, 0).unwrap().1;
        let (_, _, cols, rows) = find_resize_marker(bytes, first_end).expect("second marker");
        assert_eq!((cols, rows), (20, 20));
    }

    #[test]
    fn parse_resize_marker_dims_accepts_well_formed_body() {
        assert_eq!(parse_resize_marker_dims(b"120;48"), Some((120, 48)));
    }

    #[test]
    fn parse_resize_marker_dims_rejects_non_numeric() {
        assert_eq!(parse_resize_marker_dims(b"abc;48"), None);
        assert_eq!(parse_resize_marker_dims(b"120;xyz"), None);
    }

    #[test]
    fn parse_resize_marker_dims_rejects_missing_field() {
        assert_eq!(parse_resize_marker_dims(b"120"), None);
        assert_eq!(parse_resize_marker_dims(b""), None);
    }

    // ── task0003 AC-2 (D2, review round-2 finding `602e685494248cbb`):
    // encoder and decoder share ONE accepted dimension range ─────────────

    /// A dimension just past the OLD (private, pre-task0003) decoder cap of
    /// 2000 — previously silently rejected even though the encoder accepted
    /// it — is now accepted, because both sides read the same
    /// `RESIZE_MARKER_MAX_COLS` / `RESIZE_MARKER_MAX_ROWS` constants.
    #[test]
    fn parse_resize_marker_dims_accepts_value_past_old_hardcoded_2000_cap() {
        assert_eq!(parse_resize_marker_dims(b"2500;2500"), Some((2500, 2500)));
    }

    /// A dimension still beyond the (now-shared, wider) accepted range is
    /// rejected — the range widened, but it did not disappear.
    #[test]
    fn parse_resize_marker_dims_still_rejects_dimension_past_shared_max() {
        assert_eq!(
            parse_resize_marker_dims(
                format!("{};{}", RESIZE_MARKER_MAX_COLS + 1, RESIZE_MARKER_MAX_ROWS).as_bytes()
            ),
            None
        );
        assert_eq!(
            parse_resize_marker_dims(
                format!("{};{}", RESIZE_MARKER_MAX_COLS, RESIZE_MARKER_MAX_ROWS + 1).as_bytes()
            ),
            None
        );
    }

    /// AC-2 end-to-end: a resize past the OLD hardcoded decoder range still
    /// produces a marker that a replay honors (encoder -> decoder agree).
    #[test]
    fn reset_and_replay_honors_a_resize_past_the_old_hardcoded_decoder_range() {
        let cols: u16 = 80;
        let rows: u16 = 24;
        let wide_cols: u16 = 2500; // past the OLD 2000 decoder cap
        let mut bytes = b"before-resize\r\n".to_vec();
        bytes.extend_from_slice(&resize_marker_bytes(wide_cols, 40));
        // A cursor-addressed write near the far right edge of `wide_cols`
        // only lands correctly if the marker actually resized the core to
        // `wide_cols` before this segment is fed — an ignored marker would
        // clamp/wrap it at the caller's `cols` (80) instead.
        bytes.extend_from_slice(format!("\x1b[1;{wide_cols}Hedge").as_bytes());
        bytes.extend_from_slice(&resize_marker_bytes(cols, rows));
        bytes.extend_from_slice(b"after-resize\r\n");

        let mut core = TerminalCore::new(cols, rows, 1000);
        core.reset_and_replay(&bytes);

        assert_eq!(core.cols(), cols, "core must end back at target size");
        assert_eq!(core.rows(), rows);
    }

    // ── clamp_resize_dims (task0003 D2) ───────────────────────────────────

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

    // ── replay_with_resize_markers / reset_and_replay marker interpretation ─

    /// A single mid-stream marker resizes the core between the two
    /// segments, and the core ends back at its ORIGINAL (caller-requested)
    /// dimensions.
    #[test]
    fn reset_and_replay_resizes_mid_stream_and_restores_target_dims() {
        let mut core = TerminalCore::new(80, 24, 1000);
        let mut bytes = b"before-resize\r\n".to_vec();
        bytes.extend_from_slice(&resize_marker(40, 10));
        bytes.extend_from_slice(b"after-resize\r\n");
        core.reset_and_replay(&bytes);
        // Core ends back at the dims it was constructed with (80, 24), not
        // the marker's intermediate (40, 10).
        assert_eq!(core.cols(), 80);
        assert_eq!(core.rows(), 24);
        assert!(
            core.get_line_text(0).contains("before-resize"),
            "content before the marker must still be present"
        );
        assert!(
            core.get_line_text(1).contains("after-resize"),
            "content after the marker must still be present"
        );
    }

    /// Test-only helper mirroring `mux::scrollback_buffer::resize_marker_bytes`'s
    /// byte format, used directly by `terminal_core` unit tests (which have
    /// no dependency on the `emterm` mux crate).
    fn resize_marker(cols: u16, rows: u16) -> Vec<u8> {
        format!("\x1b]777;emterm;resize;{cols};{rows}\x07").into_bytes()
    }

    /// review round-1 rework, finding `6ff208bbc674189c` (high) / task0002
    /// AC-5: N consecutive resize markers with NO bytes between them cost
    /// at most TWO reflows total — one to apply the last pending size right
    /// before the trailing non-empty segment, and one for the mandatory
    /// final restore back to the caller's target size (since this run's
    /// last marker's dims differ from the target) — never one reflow PER
    /// MARKER. Observed directly via `reflow_call_count`. Pre-fix, each of
    /// the 5 markers triggered its own immediate `resize()` call regardless
    /// of whether any bytes ever followed it at that size, for 5 (marker)
    /// + 1 (final restore) = 6 reflows on this same recording.
    #[test]
    fn replay_coalesces_consecutive_markers_into_a_single_reflow() {
        let mut core = TerminalCore::new(80, 24, 1000);
        let before = core.reflow_call_count;
        let mut bytes = b"before\r\n".to_vec();
        // Five consecutive markers, no bytes between any of them.
        for (cols, rows) in [(40, 10), (100, 30), (60, 20), (120, 40), (90, 25)] {
            bytes.extend_from_slice(&resize_marker(cols, rows));
        }
        bytes.extend_from_slice(b"after\r\n");
        core.reset_and_replay(&bytes);
        let reflows = core.reflow_call_count - before;
        assert_eq!(
            reflows, 2,
            "a run of 5 consecutive markers followed by ONE non-empty segment \
             must reflow at most twice (last pending size + final restore-to- \
             target), never once per marker (which would be 6 here)"
        );
        assert_eq!(core.cols(), 80, "core must end back at the target size");
        assert_eq!(core.rows(), 24);
        assert!(core.get_line_text(0).contains("before"));
        assert!(core.get_line_text(1).contains("after"));
    }

    /// AC-5 (zero-reflow edge case): if a run of consecutive markers is
    /// never followed by any bytes at all (the recording ends right after
    /// the last marker), NO reflow happens for any of them — there is
    /// nothing to preserve content for, and the core never actually left
    /// its target size, so even the final restore-to-target check is a
    /// no-op.
    #[test]
    fn replay_trailing_consecutive_markers_with_no_following_bytes_reflow_zero_times() {
        let mut core = TerminalCore::new(80, 24, 1000);
        let before = core.reflow_call_count;
        let mut bytes = b"only-content\r\n".to_vec();
        for (cols, rows) in [(40, 10), (100, 30), (60, 20)] {
            bytes.extend_from_slice(&resize_marker(cols, rows));
        }
        // No bytes after the last marker.
        core.reset_and_replay(&bytes);
        assert_eq!(
            core.reflow_call_count - before,
            0,
            "a trailing marker run with nothing fed at any of its sizes must \
             cost zero reflows"
        );
        assert_eq!(core.cols(), 80);
        assert_eq!(core.rows(), 24);
        assert!(core.get_line_text(0).contains("only-content"));
    }

    /// AC-5 (grid-equivalence variant, the ACs's stated alternative to a
    /// counter): a run of consecutive markers ending in dims (D) followed
    /// by content must produce a grid IDENTICAL to a recording containing
    /// only the SINGLE final marker (D) followed by the same content — the
    /// intermediate dimensions leave no observable trace either way.
    #[test]
    fn replay_consecutive_markers_grid_matches_single_final_dimension_case() {
        let mut multi = TerminalCore::new(80, 24, 1000);
        let mut multi_bytes = b"before\r\n".to_vec();
        for (cols, rows) in [(40, 10), (100, 30), (60, 20)] {
            multi_bytes.extend_from_slice(&resize_marker(cols, rows));
        }
        multi_bytes.extend_from_slice(b"after\r\n");
        multi.reset_and_replay(&multi_bytes);

        let mut single = TerminalCore::new(80, 24, 1000);
        let mut single_bytes = b"before\r\n".to_vec();
        single_bytes.extend_from_slice(&resize_marker(60, 20));
        single_bytes.extend_from_slice(b"after\r\n");
        single.reset_and_replay(&single_bytes);

        assert_eq!(grid_fingerprint(&multi), grid_fingerprint(&single));
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
        let built = TerminalCore::build_from_snapshot(80, 24, 1000, &payload, &never)
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
        let built =
            TerminalCore::build_from_snapshot(80, 24, 100, b"", &never).expect("not cancelled");

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
        let built = TerminalCore::build_from_snapshot(80, 24, 100, &payload, &never)
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
            TerminalCore::build_from_snapshot(80, 24, 100, &payload, &NEVER)
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
            TerminalCore::build_from_snapshot(80, 24, 100, &payload, &cancelled).is_none(),
            "a pre-set cancel flag must abandon the build"
        );

        let live = std::sync::atomic::AtomicBool::new(false);
        assert!(
            TerminalCore::build_from_snapshot(80, 24, 100, &payload, &live).is_some(),
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
        let built = TerminalCore::build_from_snapshot(80, 24, scrollback_lines, &payload, &never)
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
        let built = TerminalCore::build_from_snapshot(80, 24, small_c, &payload, &never)
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
        let built = TerminalCore::build_from_snapshot(80, 10, small_c, &payload, &never)
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
        let built = TerminalCore::build_from_snapshot(80, 24, 100, &payload, &never)
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
            TerminalCore::build_scrollback_only_from_snapshot(80, 24, 100, &payload, &never)
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
            TerminalCore::build_scrollback_only_from_snapshot(80, 24, 100, &payload, &never)
                .expect("reference build not cancelled");

        // Production path: bypass-on 1st-pass + bypass-off 2nd-pass + merge.
        let bypass_replay =
            TerminalCore::build_from_snapshot(80, 24, 100, &payload, &never).expect("1st-pass");
        let mut live = bypass_replay.core;
        // Bypass leaves scrollback empty by design.
        assert_eq!(live.scrollback_count(), 0);
        let rebuilt =
            TerminalCore::build_scrollback_only_from_snapshot(80, 24, 100, &payload, &never)
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
        // A handful of lines that fit within the small viewport with no
        // eviction yet, so the upcoming grow needs no history bypass would
        // have discarded.
        for i in 0..5u32 {
            payload.extend_from_slice(format!("early line {i}\r\n").as_bytes());
        }
        payload.extend_from_slice(&resize_marker(cols, grown_rows));
        // Bulk content at the grown size — large enough to comfortably
        // exceed 64 KiB and to populate substantial (virtual, under
        // bypass) scrollback before the shrink.
        for i in 0..3000u32 {
            payload.extend_from_slice(
                format!("grown-size scroll line {i} padded for size\r\n").as_bytes(),
            );
        }
        // The row-count-SHRINKING marker under test (AC-7): pushes the
        // rows that no longer fit the smaller viewport into scrollback —
        // exactly the content-preserving reflow finding `1698d9b52a89e241`
        // is about.
        payload.extend_from_slice(&resize_marker(cols, small_rows));
        // A few lines after the shrink so the segment following the marker
        // is non-empty (the marker is actually applied) — this also
        // leaves the core already at `small_rows`, matching the
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
            cols, small_rows, 5000, &payload, &never,
        )
        .expect("reference build not cancelled");

        // Under test: `build_from_snapshot` — D6 downgrades it out of the
        // bypass for this payload (it contains a growing marker), so its
        // result alone must already match the synchronous reference.
        let bypass_replay =
            TerminalCore::build_from_snapshot(cols, small_rows, 5000, &payload, &never)
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
        // Constructed AT the grown size, so the FIRST marker (below) is a
        // shrink — history then accumulates in scrollback at the small
        // size (this is also where the bulk padding lives, so the payload
        // comfortably clears the 64 KiB off-thread bypass threshold), and
        // the SECOND marker grows back to the construction size (a no-op
        // for the implicit final restore). Nothing follows the grow, so
        // the fingerprint comparison below looks at the just-grown
        // viewport directly — content added AFTER the grow would scroll
        // the transient post-grow state out of view before this test ever
        // inspects it, hiding the divergence being tested for.
        payload.extend_from_slice(&resize_marker(cols, small_rows));
        // History produced at the SMALL size, comfortably more than
        // `small_rows` so there is real content sitting in scrollback for
        // the upcoming growth to pull back up into the viewport.
        for i in 0..3000u32 {
            payload.extend_from_slice(
                format!("small-size scroll line {i} padded for size\r\n").as_bytes(),
            );
        }
        // The row-count-GROWING marker under test (AC-9): the viewport
        // widens back to the construction size and should pull rows back
        // up from the scrollback history just produced above.
        payload.extend_from_slice(&resize_marker(cols, grown_rows));
        assert!(
            payload.len() >= 64 * 1024,
            "payload must be >= 64 KiB to match AC-9's bypass-path scenario, got {}",
            payload.len()
        );

        let never = std::sync::atomic::AtomicBool::new(false);

        // Reference: synchronous (non-bypass) path.
        let reference = TerminalCore::build_scrollback_only_from_snapshot(
            cols, grown_rows, 5000, &payload, &never,
        )
        .expect("reference build not cancelled");

        // Under test: bypass path (`build_from_snapshot`) — D6 downgrades
        // out of the bypass for this payload because it contains a
        // row-growing marker.
        let bypass_replay =
            TerminalCore::build_from_snapshot(cols, grown_rows, 5000, &payload, &never)
                .expect("bypass-path build not cancelled");

        assert_eq!(
            grid_fingerprint(&bypass_replay.core),
            grid_fingerprint(&reference.core),
            "bypass-path viewport must match the synchronous path across a row-growing marker"
        );
    }

    /// A marker that changes only COLS — rows stay constant throughout,
    /// including at the implicit final restore-to-target — does not trigger
    /// the D6 downgrade: `build_from_snapshot` still returns an EMPTY
    /// scrollback, matching the existing (pre-D6) bypass invariant other
    /// tests already rely on. (A genuine ROW shrink cannot serve as this
    /// counter-example: `replay_with_resize_markers` always restores to the
    /// construction/target row count at the end of the drain, so a payload
    /// that ever shrinks rows always ends up growing back — explicitly or
    /// implicitly — by the time the drain finishes.)
    #[test]
    fn build_from_snapshot_stays_bypassed_for_a_cols_only_marker() {
        let cols_a: u16 = 80;
        let cols_b: u16 = 40;
        let rows: u16 = 24;
        let mut payload: Vec<u8> = Vec::new();
        for i in 0..5u32 {
            payload.extend_from_slice(format!("early line {i}\r\n").as_bytes());
        }
        // Only cols changes; rows stay the same throughout — D6 must not
        // fire (it is scoped to row growth, not cols).
        payload.extend_from_slice(&resize_marker(cols_b, rows));
        for i in 0..3000u32 {
            payload.extend_from_slice(
                format!("padding line {i} padded for size padded padded\r\n").as_bytes(),
            );
        }
        assert!(payload.len() >= 64 * 1024);

        let never = std::sync::atomic::AtomicBool::new(false);
        let bypass_replay = TerminalCore::build_from_snapshot(cols_a, rows, 5000, &payload, &never)
            .expect("bypass-path build not cancelled");
        assert_eq!(
            bypass_replay.core.scrollback_count(),
            0,
            "a marker that only changes cols (rows constant) must still take the bypass fast path"
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
