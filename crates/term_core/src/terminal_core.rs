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
        // D10 (mux-tab-switch-bypass-refix task0001, review finding
        // `b6a60c440da70e79`): the actually measured bug shape's MIDDLE is
        // 26 segments, one past the round-8 gate's then-current bound (24)
        // — see `BYPASS_PREFIX_MAX_SEGMENTS`'s doc for why that bound had
        // silently drifted stale (the daemon-side cap it mirrors moved to
        // 62 in a later round without this gate following) and why
        // realigning it to the daemon's CURRENT cap does not reintroduce
        // the cost this bound exists to bound (NFR1).
        //
        // D11 (task0004, review round-1 rework, findings `a1a06ed541045dd5`
        // / `77da6aceb73b1a72`): D10's "same-width by construction" cost
        // rationale for 62 holds ONLY when `h > 0` — `middle_is_row_bounded`
        // has, by construction of `candidate_safe` above, already verified
        // every one of `segments[h..k]` is same-width in that case. When
        // `h == 0` (D9's fold-degradation path), NOTHING has verified
        // that — `segments[0..k]` can contain column-changing entries,
        // each paying `resize_full_reflow` (cost proportional to the
        // content accumulated within the MIDDLE so far) instead of the
        // row-delta-bounded `resize_same_width`. Apply the tighter,
        // independently-justified `BYPASS_PREFIX_MAX_SEGMENTS_UNFOLDED_HEAD`
        // bound on that path instead — see its own doc and
        // `BYPASS_PREFIX_MAX_SEGMENTS`'s doc (both constants' cost budget
        // is shared with, and bounded by, `BYPASS_PREFIX_MAX_BYTES` below
        // regardless of tier).
        let segment_bound = if h > 0 {
            BYPASS_PREFIX_MAX_SEGMENTS
        } else {
            BYPASS_PREFIX_MAX_SEGMENTS_UNFOLDED_HEAD
        };
        let bypass_split = bypass
            && k > 0
            && middle_segment_count <= segment_bound
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
            actions.extend(
                match core.replay_segments(suffix_bytes, &[], cancel, cols, rows) {
                    Some(a) => a,
                    None => {
                        // Cancelled mid-drain: leave the core consistent before
                        // discarding it via the `None` return (matches the
                        // non-split path below).
                        core.disable_snapshot_bypass();
                        return None;
                    }
                },
            );
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
pub(in crate::terminal_core) fn stable_target_suffix_start(
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
pub(in crate::terminal_core) fn leading_uniform_run_len(target_cols: u16, segments: &[ReplaySegment]) -> (usize, u16) {
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
pub(in crate::terminal_core) fn middle_is_row_bounded(target_cols: u16, head_rows: u16, middle: &[ReplaySegment]) -> bool {
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
pub(in crate::terminal_core) const BYPASS_SUFFIX_MIN_BYTES: usize = 4096;

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
///
/// D11 cross-reference (task0004, review round-1 rework, findings
/// `a1a06ed541045dd5` / `77da6aceb73b1a72`): this bound and
/// [`BYPASS_PREFIX_MAX_SEGMENTS`] / [`BYPASS_PREFIX_MAX_SEGMENTS_UNFOLDED_HEAD`]
/// form ONE cost budget for the MIDDLE, not two independent ones — the
/// segment bounds exist because a MIDDLE built from many small segments
/// still pays one reflow per segment regardless of how little of this
/// byte budget it uses (see their own docs), so raising this byte bound
/// changes what a single reflow at the segment bounds' worst case costs.
/// Re-measure both together (a release bench in `bench.rs`) before
/// changing either.
pub(in crate::terminal_core) const BYPASS_PREFIX_MAX_BYTES: usize = 64 * 1024;

/// Maximum number of segments the MIDDLE (`segments[h..k]`, per
/// [`leading_uniform_run_len`]) may contain for
/// `build_from_snapshot_inner`'s D1''' prefix/suffix split to be worth
/// engaging (D5''''', round-8 rework, review round-7 finding
/// `a4f4e36fef377d05`).
///
/// [`BYPASS_PREFIX_MAX_BYTES`] bounds the prefix's total BYTE length, but a
/// prefix built from many small segments still pays one full,
/// content-preserving reflow PER SEGMENT (`replay_segments`'s per-segment
/// resize), regardless of how few total bytes those segments cover — a
/// resize storm packs up to the daemon's own `MAX_DIM_MARKERS` (62, kept as
/// a literal here — `term_core` has no dependency on the mux daemon crate;
/// see `mux_ipc::protocol::MAX_SEGMENTS`'s doc for the same duplication) worth
/// of segments into a comparatively small byte span. Bounding segment COUNT
/// independent of byte length keeps that shape from silently slipping
/// through the byte-only gate.
///
/// D7 amendment (task0001, prior feature `mux-tab-switch-replay-latency`):
/// checked against `middle_segment_count` (`k - h`, per
/// [`leading_uniform_run_len`]), not the raw `k` — when there is no
/// rescuable HEAD (`h == 0`), `middle_segment_count == k` and this is
/// byte-identical to the original check.
///
/// D10 (mux-tab-switch-bypass-refix task0001, review finding
/// `b6a60c440da70e79`): raised from 24 to 62. `MAX_DIM_MARKERS` (the
/// daemon-side cap this bound duplicates as a literal, above) was
/// independently raised from 24 to 62 in a later round (see
/// `bench.rs::DAEMON_SEGMENT_CAP`'s doc) without updating THIS gate, so it
/// silently drifted stale at the daemon's PRIOR cap — the actually measured
/// bug shape (a 26-segment MIDDLE, comfortably under the daemon's CURRENT
/// 62-marker cap) was rejected by a bound calibrated to a cap the daemon no
/// longer enforces. Realigning the two restores the intended invariant
/// (this gate never rejects a shape the daemon itself could not have
/// produced) and, as a direct consequence, admits the measured shape.
///
/// D11 (task0004, review round-1 rework, findings `a1a06ed541045dd5` /
/// `77da6aceb73b1a72` / `474e01ad8c29e7f0` / `96f7205be52fece8` /
/// `1adb07864f11618f`): this bound applies ONLY on the `h > 0` (HEAD-fold-
/// succeeded) path — see the tiering at this constant's call site in
/// `build_from_snapshot_inner`. The `h == 0` path (D9's fold-degradation
/// case) uses the separate, tighter
/// [`BYPASS_PREFIX_MAX_SEGMENTS_UNFOLDED_HEAD`] instead; see that
/// constant's doc for why. The paragraph below — "every MIDDLE segment
/// transition is a SAME-WIDTH resize" — is true ONLY of the `h > 0` tier
/// this constant now exclusively covers: `middle_is_row_bounded` (this
/// gate's companion safety check) has, by the time `h` is set to a
/// nonzero `candidate_h`, already verified `cols == target_cols` for
/// every one of `segments[h..k]`. Before D11, this bound applied
/// regardless of `h`, and the same doc sentence was FALSE for `h == 0`: a
/// column change does not degrade the whole HEAD fold and stop there — it
/// degrades `h` to `0` and MIDDLE (now `segments[0..k]`) still reaches
/// this gate, unchecked for column changes (`a1a06ed541045dd5`,
/// corroborated from the performance angle by `77da6aceb73b1a72`).
///
/// Top-end derivation (`474e01ad8c29e7f0`): 62 is not an arbitrary mirror
/// of the daemon's dim-marker record cap (`MAX_DIM_MARKERS`) — it is the
/// largest MIDDLE a fold-succeeded (`h > 0`) split can EVER contain for a
/// legal daemon snapshot. A daemon snapshot carries at most
/// `mux_ipc::protocol::MAX_SEGMENTS` (64) segments; a fold-succeeded
/// MIDDLE can never claim the mandatory HEAD (`candidate_h > 0`, at least
/// 1 segment) nor the mandatory SUFFIX (a split needs `k <
/// segments.len()`, at least 1 segment past the MIDDLE) — so the ceiling
/// is `MAX_SEGMENTS - 2` = 62, exactly this constant's value. The
/// `h == 0` tier is NOT bound by this same top-end reasoning (a `h == 0`
/// MIDDLE can legally reach 63, one shy of the wire cap, since it does
/// not need to give up a HEAD slot) — [`BYPASS_PREFIX_MAX_SEGMENTS_UNFOLDED_HEAD`]
/// is deliberately smaller than that reachable ceiling, as a COST choice,
/// not a shape-completeness one; see its own doc for the excluded top
/// slots this implies.
///
/// Purpose (`96f7205be52fece8` / `1adb07864f11618f`): for the `h > 0`
/// tier, this constant's PRIMARY role is now the cap-mirror derived above
/// (pinned exactly, see `bypass_prefix_max_segments_pin` below) — its
/// cost-bound role (below) is secondary, since `resize_same_width`'s
/// row-delta-bounded cost stays cheap even at the full 62. It is not dead
/// code even so: `build_from_snapshot_inner` accepts `segments` from any
/// caller, not only a daemon-shaped one (`term_core` has no runtime
/// dependency on `mux_ipc` — NFR5), so this condition still rejects an
/// `h > 0` MIDDLE built from more than 62 segments regardless of whether a
/// real daemon could ever produce one (e.g. test-constructed or
/// otherwise non-daemon-shaped input).
///
/// Why the cost stays cheap regardless (the rationale this bound was
/// originally introduced for: "each MIDDLE segment pays one reflow
/// regardless of its byte size"): every MIDDLE segment transition in the
/// `h > 0` tier is a SAME-WIDTH resize (see the D11 paragraph above).
/// `TerminalCore::resize_same_width` (`reflow.rs`,
/// D1, round-10 rework, `mux-render-corruption` task0010) bounds a
/// same-width resize's cost to the ROW-COUNT DELTA between the two
/// dimensions, not the size of scrollback accumulated so far — the
/// per-segment cost this bound exists to cap was, at the time of the
/// original 24-segment cap, dominated by an O(accumulated-content) reflow
/// that round-10 eliminated for exactly this same-width shape (see
/// `bench.rs::segment_bounded_replay_bench_950kib_stays_bounded_at_the_daemon_cap`'s
/// doc for the re-measured numbers: tens to ~164 ms across the 24-62
/// segment range post-round-10, vs. seconds pre-round-10).
/// [`BYPASS_PREFIX_MAX_BYTES`] and the suffix-dominance check (`suffix_len
/// >= middle_len`, NFR1, IMPLEMENTATION.md D-B) still bound the MIDDLE's
/// total byte cost regardless of how many segments it is split across, so
/// a genuinely expensive MIDDLE/prefix is rejected by those gates
/// independent of this one.
pub(in crate::terminal_core) const BYPASS_PREFIX_MAX_SEGMENTS: usize = 62;

/// Maximum number of segments the MIDDLE may contain when the HEAD fold
/// did NOT succeed (`h == 0` — D9's fold-degradation path in
/// `build_from_snapshot_inner`: a column change, a MIDDLE row count
/// exceeding the HEAD's own run rows, or an insufficient HEAD run row
/// count degrades `h` all the way to `0`).
///
/// D11 (task0004, review round-1 rework, findings `a1a06ed541045dd5` /
/// `77da6aceb73b1a72`): split out of the single pre-D11
/// [`BYPASS_PREFIX_MAX_SEGMENTS`] bound. On the `h == 0` path, NOTHING has
/// verified the MIDDLE is same-width — `segments[0..k]` may contain
/// column-changing entries, each paying `TerminalCore::resize_full_reflow`
/// (cost proportional to the content accumulated in the (freshly
/// constructed, since `head_len == 0` here) core so far, i.e. bounded by
/// [`BYPASS_PREFIX_MAX_BYTES`] = 64 KiB total across the whole MIDDLE)
/// instead of the row-delta-bounded `resize_same_width`. Admitting up to
/// 62 segments on this path (pre-D11's mistaken uniform bound) means up
/// to 62 full reflows of that accumulated content, not 24 — an increase
/// this constant exists to undo.
///
/// The value (24) is deliberately the SAME value this gate used,
/// unconditionally, before D10 raised it to 62 — a value already
/// exercised (this file's `h == 0` boundary tests below) and, per this
/// bound's own historical role (identical reasoning to
/// [`BYPASS_PREFIX_MAX_BYTES`]'s doc: a bound small enough that even a
/// full non-bypass reflow of the content under it does not matter) does
/// not need new evidence to justify keeping it. It is a COST-POLICY
/// choice, independent of [`mux_ipc::protocol::MAX_SEGMENTS`] — unlike
/// [`BYPASS_PREFIX_MAX_SEGMENTS`], it carries NO wire-cap pin (see
/// `bypass_prefix_max_segments_pin`'s doc): raising or lowering it is a
/// deliberate cost decision, not drift, and a daemon snapshot's `h == 0`
/// MIDDLE could legally reach 63 (one shy of the wire cap) — the slots
/// between 24 and 63 are deliberately left out of scope on this path
/// (`474e01ad8c29e7f0`'s top-end concern, weakened here rather than made
/// true: the "this gate never rejects a shape the daemon itself could
/// have produced" invariant holds only for the `h > 0` tier above, not
/// this one).
pub(in crate::terminal_core) const BYPASS_PREFIX_MAX_SEGMENTS_UNFOLDED_HEAD: usize = 24;

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
