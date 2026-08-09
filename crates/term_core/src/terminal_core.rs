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

mod types;
pub use types::*;

mod replay;

mod replay_plan;
use replay_plan::*;

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
    /// Ordered pending device-response store (tmux-startup-query-response-leak
    /// task0002, review-round-1 rework, D5). [`Self::write_response`]
    /// APPENDS to this — never overwrites — so a single parse pass that
    /// dispatches N device queries (DA1/DA2/DSR/XTWINOPS/DECRPM)
    /// accumulates all N replies, concatenated in synthesis order.
    /// [`Self::take_response`] drains and clears it; a drain whose result
    /// is discarded (the snapshot/replay paths) removes everything
    /// pending. A plain growable `Vec<u8>` replaces the pre-task0002
    /// fixed 64-byte single slot: growth is bounded only by the input
    /// that produced it (the existing 1 MiB / 12 ms parse-chunk coalesce
    /// bound in `tabs.rs`), not by a fixed capacity, so no in-scope
    /// response is ever silently dropped.
    pub(crate) response_queue: Vec<u8>,
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
            // Sprint 4 (task0002 D5: ordered append-only store, see field doc)
            response_queue: Vec::new(),
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
        self.response_queue.clear();
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

/// Test-time pin: `BYPASS_PREFIX_MAX_SEGMENTS` — the `h > 0` (fold-
/// succeeded) tier's bound ONLY (see that constant's D11 doc) — must equal
/// the largest MIDDLE a legal daemon snapshot can ever produce for that
/// tier, derived exactly (not approximated) from
/// `mux_ipc::protocol::MAX_SEGMENTS` (64, the wire-level cap on segments in
/// one snapshot): a fold-succeeded MIDDLE can never claim the mandatory
/// HEAD (`candidate_h > 0`, at least 1 segment) nor the mandatory SUFFIX (a
/// split needs `k < segments.len()`, at least 1 segment past the MIDDLE),
/// so the ceiling is `MAX_SEGMENTS - MANDATORY_HEAD_AND_SUFFIX_SEGMENTS` =
/// `64 - 2` = 62.
///
/// D11 (task0004, review round-1 rework, findings `474e01ad8c29e7f0` /
/// `96f7205be52fece8` / `1adb07864f11618f`): replaces the pre-D11
/// asymmetric upper/lower-bound pair (`<= MAX_SEGMENTS`, `+ WIRE_CAP_SLACK
/// >= MAX_SEGMENTS`) with a single equality assertion — the exact
/// derivation above makes an inequality unnecessary and catches drift in
/// EITHER direction. `WIRE_CAP_SLACK`'s old rationale ("2 synthesized
/// segments a daemon snapshot can carry beyond the raw dim-marker count")
/// produced the SAME number but the wrong reasoning for this constant's
/// post-D11 scope (it never accounted for the mandatory HEAD a
/// fold-succeeded MIDDLE gives up) — `474e01ad8c29e7f0`'s off-by-one
/// concern. Renamed to `MANDATORY_HEAD_AND_SUFFIX_SEGMENTS` to state the
/// derivation this pin actually enforces.
///
/// This pin covers ONLY `BYPASS_PREFIX_MAX_SEGMENTS`.
/// [`BYPASS_PREFIX_MAX_SEGMENTS_UNFOLDED_HEAD`] (the `h == 0` tier) is a
/// genuinely independent cost-policy value — see its own doc — and
/// deliberately carries NO such pin: changing it, in either direction, is
/// a design decision, not drift, and must not fail this test
/// (`96f7205be52fece8`). A revert of `BYPASS_PREFIX_MAX_SEGMENTS` itself
/// back to 24 without a matching decision IS still exactly the round-7/
/// round-8 regression this pin's assertion message names — that
/// diagnosis is now accurate precisely because it no longer also fires
/// for a deliberate change to the OTHER (unpinned) constant
/// (`1adb07864f11618f`).
///
/// Empirically verified (task0004 implementer, matching how loop 2's own
/// pin was verified): with `BYPASS_PREFIX_MAX_SEGMENTS` temporarily set to
/// `24`, `bypass_prefix_max_segments_matches_the_fold_succeeded_ceiling`
/// FAILS with `left: 24, right: 62` and the assertion message above
/// (round-7/round-8 regression diagnosis, correctly worded); restored to
/// `62`, the full `--lib` suite (775 tests) passes.
#[cfg(test)]
mod bypass_prefix_max_segments_pin;

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
mod tests;
