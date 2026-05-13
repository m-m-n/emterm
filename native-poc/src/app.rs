//! Top-level `App` state container.
//!
//! Phase 1 was empty. Phase 2 holds a PTY-backed tab and offers an
//! `on_resize` hook that propagates the new grid dimensions to all PTYs.
//! Sub-phase 2 adds dirty-row tracking state (cursor + selection history,
//! `needs_full_redraw` flag, `EMTERM_FULL_REDRAW` toggle) used by the
//! renderer to skip frames that wouldn't change a pixel.
//! Later phases extend this with a viewer registry and settings.

use std::sync::Arc;
use std::time::Instant;

use term_core::terminal_core::TerminalCore;

use crate::selection::Selection;
use crate::settings::Settings;
use crate::tabs::Tab;

/// Cursor blink half-period in milliseconds. 530 ms matches xterm's
/// `cursorBlinkXOR` interval; one full on/off cycle is `2 * BLINK_HALF_MS`.
pub const BLINK_HALF_MS: u128 = 530;

/// Where the viewport currently sits relative to the live tail.
///
/// `Live` means the user is tracking new output (auto-follow). When PTY
/// output arrives in this state, the viewport advances with it.
///
/// `OffsetFromLive(n)` means the user has scrolled back `n` rows into
/// scrollback. New PTY output preserves this offset so the user does not
/// get yanked back to the bottom mid-read.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum ScrollPosition {
    #[default]
    Live,
    OffsetFromLive(u32),
}

pub struct App {
    pub tabs: Vec<Tab>,
    pub active: usize,
    /// Last known grid size in cells. Updated by `window_host` whenever the
    /// window resizes; PTYs are resized to match.
    pub cell_size: GridDims,
    /// Active mouse selection on the active tab, if any. Phase 4 owns this;
    /// later phases may move it per-tab when tabs preserve selection.
    pub selection: Option<Selection>,
    /// Runtime settings (ambiguous-width policy, OSC 52 policy, and
    /// future fields). Loaded from `settings.json` in Phase 7; today
    /// initialized to default. Wrapped in `Arc` so the per-tab
    /// `NativeCallbacks` can share an immutable view without copying.
    pub settings: Arc<Settings>,
    /// Scrollback position. `Live` = auto-follow; `OffsetFromLive(n)` = the
    /// viewport is pinned `n` rows above the live tail.
    pub scroll_position: ScrollPosition,
    /// Whether the alternate screen buffer (DECSET 1049/47/1047) is active.
    /// Tracked by draining `core.take_mode_actions()` after every chunk in
    /// `Tab::pump`. While true, scrollback inputs are suppressed and the
    /// position is pinned to `Live`.
    pub alt_screen: bool,
    /// Reference point for cursor-blink phase computation.
    blink_started: Instant,
    /// Cursor-blink "visible" phase observed during the previous render.
    /// When the phase flips, the cursor row joins the dirty union so the
    /// renderer can paint/erase the cursor overlay.
    previous_blink_visible: bool,
    /// Cursor row/col from the previous rendered frame. The renderer dirties
    /// this row so a moved cursor doesn't ghost the old position.
    previous_cursor: Option<(u16, u16)>,
    /// Selection from the previous rendered frame. Vacated selection rows
    /// must be repainted to clear highlight.
    previous_selection: Option<Selection>,
    /// Set on construction, resize, and surface recovery. Forces the next
    /// frame to repaint every row regardless of `term_core` dirty bits.
    needs_full_redraw: bool,
    /// Debug toggle (env `EMTERM_FULL_REDRAW=1`) that permanently disables
    /// the dirty-row optimization for triage.
    force_full_redraw: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GridDims {
    pub cols: u16,
    pub rows: u16,
}

impl Default for GridDims {
    fn default() -> Self {
        Self { cols: 80, rows: 24 }
    }
}

impl App {
    pub fn new() -> Self {
        let force_full_redraw = std::env::var("EMTERM_FULL_REDRAW")
            .ok()
            .map(|v| v == "1")
            .unwrap_or(false);
        if force_full_redraw {
            log::warn!("EMTERM_FULL_REDRAW=1: dirty-row optimization disabled");
        }
        Self {
            tabs: Vec::new(),
            active: 0,
            cell_size: GridDims::default(),
            selection: None,
            settings: Arc::new(Settings::new()),
            scroll_position: ScrollPosition::Live,
            alt_screen: false,
            blink_started: Instant::now(),
            previous_blink_visible: true,
            previous_cursor: None,
            previous_selection: None,
            needs_full_redraw: true,
            force_full_redraw,
        }
    }

    /// True when the cursor should currently render its glyph. When the
    /// terminal disables blink the cursor is always considered visible
    /// here (terminal-visibility is gated separately in `draw_cursor`).
    pub fn blink_visible_now(&self, blink_enabled: bool) -> bool {
        if !blink_enabled {
            return true;
        }
        let phase = self.blink_started.elapsed().as_millis() / BLINK_HALF_MS;
        phase.is_multiple_of(2)
    }

    /// Reset the blink reference to "now" so the cursor enters its visible
    /// half-cycle. Use this when the user does something that should
    /// re-pin attention to the cursor (typing, paste, tab switch).
    #[allow(dead_code)]
    pub fn reset_blink_phase(&mut self) {
        self.blink_started = Instant::now();
        self.previous_blink_visible = true;
    }

    /// Spawn the initial shell tab. Called once at startup.
    pub fn spawn_initial_tab(&mut self) {
        let dims = self.cell_size;
        let tab = Tab::spawn_shell(
            "shell",
            dims.cols,
            dims.rows,
            self.settings.scrollback_lines,
            self.settings.clone(),
        );
        self.tabs.push(tab);
        self.active = 0;
        // A brand-new tab populated rows; ensure the first frame draws them.
        self.needs_full_redraw = true;
    }

    /// Spawn an additional shell tab, switch to it, and request a
    /// repaint. Used by `AppAction::NewTab` and `TabEvent::New`.
    pub fn spawn_new_tab(&mut self) {
        let dims = self.cell_size;
        let tab = Tab::spawn_shell(
            "shell",
            dims.cols,
            dims.rows,
            self.settings.scrollback_lines,
            self.settings.clone(),
        );
        self.tabs.push(tab);
        self.active = self.tabs.len() - 1;
        self.needs_full_redraw = true;
    }

    /// Close the tab at `idx`. Returns `true` when the close emptied
    /// the tabs vector, signaling the app loop that the window should
    /// exit. `tabs.is_empty()` is the same signal; this is a
    /// convenience for code that needs to branch immediately after
    /// the close.
    pub fn close_tab(&mut self, idx: usize) -> bool {
        if idx >= self.tabs.len() {
            return self.tabs.is_empty();
        }
        // Drop the tab — its `PtySession::Drop` impl kills the child
        // and joins reader/writer threads.
        self.tabs.remove(idx);
        if self.tabs.is_empty() {
            self.active = 0;
            self.needs_full_redraw = true;
            return true;
        }
        if self.active >= self.tabs.len() {
            self.active = self.tabs.len() - 1;
        } else if idx < self.active {
            // Removed tab was to the left of the active one; shift left.
            self.active -= 1;
        }
        self.needs_full_redraw = true;
        false
    }

    /// Switch to the tab at `idx` (no-op for out-of-range / same idx).
    pub fn switch_to_tab(&mut self, idx: usize) {
        if idx >= self.tabs.len() || idx == self.active {
            return;
        }
        self.active = idx;
        self.needs_full_redraw = true;
    }

    /// Apply a [`crate::ui::TabEvent`] emitted by the tab bar widget.
    /// Returns `true` when the resulting state should exit the window
    /// (i.e. the last tab was closed).
    pub fn apply_tab_event(&mut self, evt: crate::ui::TabEvent) -> bool {
        match evt {
            crate::ui::TabEvent::New => {
                self.spawn_new_tab();
                false
            }
            crate::ui::TabEvent::Close(idx) => self.close_tab(idx),
            crate::ui::TabEvent::Switch(idx) => {
                self.switch_to_tab(idx);
                false
            }
        }
    }

    /// Apply a global keybind [`crate::ui::AppAction`]. Returns `true`
    /// when the resulting state should exit the window.
    pub fn apply_action(&mut self, action: crate::ui::AppAction) -> bool {
        match action {
            crate::ui::AppAction::NewTab => {
                self.spawn_new_tab();
                false
            }
            crate::ui::AppAction::CloseTab => {
                let idx = self.active;
                self.close_tab(idx)
            }
            crate::ui::AppAction::NextTab => {
                if self.tabs.is_empty() {
                    return false;
                }
                let next = (self.active + 1) % self.tabs.len();
                self.switch_to_tab(next);
                false
            }
            crate::ui::AppAction::PrevTab => {
                if self.tabs.is_empty() {
                    return false;
                }
                let prev = if self.active == 0 {
                    self.tabs.len() - 1
                } else {
                    self.active - 1
                };
                self.switch_to_tab(prev);
                false
            }
            crate::ui::AppAction::JumpTab(n) => {
                if self.tabs.is_empty() {
                    return false;
                }
                // n is 1-based and clamped to the existing range.
                let idx = (n.saturating_sub(1) as usize).min(self.tabs.len() - 1);
                self.switch_to_tab(idx);
                false
            }
        }
    }

    pub fn has_tabs(&self) -> bool {
        !self.tabs.is_empty()
    }

    /// Phase 4-D: project the active tab + the current wall clock into a
    /// [`crate::ui::status_bar::StatusBarState`] for the status-bar
    /// widget. When the active tab is attached to a mux session
    /// (`mux_session_name` is `Some` and a `StatusUpdateMsg` has been
    /// received) the `mux` field is populated; otherwise only the
    /// clock is rendered.
    pub fn status_bar_state(&self) -> crate::ui::status_bar::StatusBarState {
        use crate::ui::status_bar::{MuxStatus, StatusBarState};
        use std::time::SystemTime;

        let clock_hhmmss = crate::ui::status_bar::format_local_clock(SystemTime::now());

        let mux = self.active_tab().and_then(|t| {
            // Both pieces are required: the session name confirms the
            // tab is in mux mode, and the StatusUpdateMsg supplies the
            // window-list / right-segment strings rendered by the
            // widget. The daemon always pushes at least one update on
            // attach, so the typical attached state has both.
            match (&t.mux_session_name, &t.mux_status_state) {
                (Some(name), Some(status)) => Some(MuxStatus {
                    session_name: name.clone(),
                    status: status.clone(),
                }),
                _ => None,
            }
        });

        StatusBarState { mux, clock_hhmmss }
    }

    pub fn active_tab(&self) -> Option<&Tab> {
        self.tabs.get(self.active)
    }

    pub fn active_tab_mut(&mut self) -> Option<&mut Tab> {
        self.tabs.get_mut(self.active)
    }

    /// Drain PTY events on every tab. Returns true if any tab produced
    /// new bytes (caller schedules a redraw).
    pub fn pump_all(&mut self) -> bool {
        let mut changed = false;
        for tab in &mut self.tabs {
            // Phase 4-C (APC redesign): `Tab::pump` already routes
            // APC-encoded mux messages into the tab's own state via
            // `apply_mux_message` (see `crate::mux::apc`). There is no
            // separate `pump_mux` pass — the bridge CLI runs inside the
            // same PTY, so a single drain is sufficient.
            if tab.pump() {
                changed = true;
            }
        }
        // Mirror the active tab's alt-screen flag onto the App so the
        // scroll input routes can suppress wheel / Shift+Page during
        // alt-screen sessions.
        let active_alt = self.tabs.get(self.active).map(|t| t.alt_screen);
        if let Some(active_alt) = active_alt {
            self.set_alt_screen(active_alt);
        }
        // Notify scroll-position state machine that new bytes arrived so
        // the auto-follow rule can preserve the off-tail offset.
        if changed {
            self.on_pty_output();
        }
        // Reap exited tabs (Phase 5 will refine the policy).
        let before = self.tabs.len();
        self.tabs.retain(|t| !t.exited);
        if self.tabs.len() != before {
            if self.active >= self.tabs.len() && !self.tabs.is_empty() {
                self.active = self.tabs.len() - 1;
            }
            changed = true;
            // Tab roster changed; redraw everything to repaint the title bar
            // and the new active grid.
            self.needs_full_redraw = true;
        }
        changed
    }

    /// Propagate a new grid size to all PTYs.
    pub fn set_grid_size(&mut self, cols: u16, rows: u16) {
        self.cell_size = GridDims { cols, rows };
        for tab in &self.tabs {
            tab.resize(cols, rows);
        }
        // Resize invalidates the previously rendered framebuffer; the
        // simplest correct response is a full redraw.
        self.needs_full_redraw = true;
    }

    /// Mark the next frame as needing a full repaint regardless of the
    /// `term_core` dirty bits. Use after resize / surface recovery /
    /// other structural changes.
    pub fn mark_full_redraw(&mut self) {
        self.needs_full_redraw = true;
    }

    // ── Scrollback (Phase 4 sub-phase 4) ─────────────────────

    /// Current scrollback offset in rows. `0` means `Live`. Saturates at the
    /// configured `scrollback_lines` ceiling.
    pub fn scroll_offset(&self) -> u32 {
        match self.scroll_position {
            ScrollPosition::Live => 0,
            ScrollPosition::OffsetFromLive(n) => n,
        }
    }

    /// Scroll back by `delta` rows (clamps to `scrollback_lines`). No-op
    /// when alt-screen is active.
    pub fn scroll_up_by(&mut self, delta: u32) {
        if self.alt_screen {
            return;
        }
        let new = self.scroll_offset().saturating_add(delta);
        let max = self.settings.scrollback_lines;
        self.scroll_position = if new == 0 {
            ScrollPosition::Live
        } else {
            ScrollPosition::OffsetFromLive(new.min(max))
        };
        self.needs_full_redraw = true;
    }

    /// Scroll forward (toward live tail) by `delta` rows. Snaps to `Live`
    /// when the resulting offset would be zero. No-op when alt-screen is
    /// active.
    pub fn scroll_down_by(&mut self, delta: u32) {
        if self.alt_screen {
            return;
        }
        let cur = self.scroll_offset();
        let new = cur.saturating_sub(delta);
        self.scroll_position = if new == 0 {
            ScrollPosition::Live
        } else {
            ScrollPosition::OffsetFromLive(new)
        };
        self.needs_full_redraw = true;
    }

    /// Jump to the top of the scrollback buffer (max offset).
    pub fn scroll_to_top(&mut self) {
        if self.alt_screen {
            return;
        }
        let max = self.settings.scrollback_lines;
        self.scroll_position = if max == 0 {
            ScrollPosition::Live
        } else {
            ScrollPosition::OffsetFromLive(max)
        };
        self.needs_full_redraw = true;
    }

    /// Jump back to the live tail.
    pub fn scroll_to_live(&mut self) {
        self.scroll_position = ScrollPosition::Live;
        self.needs_full_redraw = true;
    }

    /// React to new PTY output. When already at the live tail, no-op (the
    /// renderer will pick up the new rows automatically). When sitting at an
    /// offset, preserve the offset so the user is not yanked to the bottom.
    /// Visually, the offset stays anchored because `term_core`'s ring buffer
    /// shifts the old content into scrollback under us.
    pub fn on_pty_output(&mut self) {
        // No-op for `Live`; explicit branch documents intent.
        if matches!(self.scroll_position, ScrollPosition::OffsetFromLive(_)) {
            // The offset is preserved; nothing to mutate, but we mark the
            // viewport as needing repaint because the visible row content
            // shifted by one (live tail advanced underneath the offset).
            self.needs_full_redraw = true;
        }
    }

    /// Update the alt-screen flag. Called from `Tab::pump` after draining
    /// `core.take_mode_actions()`. Entering alt-screen forces the scroll
    /// position back to `Live` (alt buffers have no scrollback).
    pub fn set_alt_screen(&mut self, active: bool) {
        if self.alt_screen == active {
            return;
        }
        self.alt_screen = active;
        if active {
            self.scroll_position = ScrollPosition::Live;
        }
        self.needs_full_redraw = true;
    }

    /// Compute the rows that must be repainted on the next frame. Union of:
    /// 1. `term_core::get_dirty_rows()` for cell-level edits
    /// 2. previous + current cursor row (to clear cursor ghost on move,
    ///    or to flip the cursor in/out of view on blink phase change)
    /// 3. previous + current selection rows (to clear highlight on shrink)
    ///
    /// Returns a sorted, deduplicated `Vec`. Returns `0..rows` when
    /// `needs_full_redraw` or `force_full_redraw` is set.
    pub fn dirty_rows_this_frame(&self, core: &TerminalCore) -> Vec<u16> {
        let rows = core.rows();
        if rows == 0 {
            return Vec::new();
        }

        if self.force_full_redraw || self.needs_full_redraw {
            return (0..rows).collect();
        }

        let mut set: Vec<u16> = core.get_dirty_rows();

        let push_unique = |set: &mut Vec<u16>, r: u16| {
            if r < rows && !set.contains(&r) {
                set.push(r);
            }
        };

        // Cursor history: previous + current. Also include the cursor row
        // when the blink phase flips so the cursor overlay can repaint or
        // erase without leaving a stale glyph.
        let cursor_row = core.get_cursor_row();
        push_unique(&mut set, cursor_row);
        if let Some((prev_row, _)) = self.previous_cursor {
            push_unique(&mut set, prev_row);
        }
        let blink_enabled = core.get_cursor_blink();
        if blink_enabled && self.blink_visible_now(blink_enabled) != self.previous_blink_visible {
            push_unique(&mut set, cursor_row);
        }

        // Selection history: union of previous + current.
        let selection_range = |sel: &Selection| -> (u16, u16) {
            let (start, end) = sel.ordered();
            (start.row, end.row)
        };
        if let Some(sel) = &self.selection {
            let (s, e) = selection_range(sel);
            for r in s..=e.min(rows - 1) {
                push_unique(&mut set, r);
            }
        }
        if let Some(sel) = &self.previous_selection {
            let (s, e) = selection_range(sel);
            for r in s..=e.min(rows - 1) {
                push_unique(&mut set, r);
            }
        }

        set.sort_unstable();
        set
    }

    /// Called after the renderer consumed the dirty set. Stores current
    /// cursor/selection/blink-phase for next-frame ghost prevention,
    /// clears the `needs_full_redraw` flag, and clears the core's dirty
    /// bits.
    pub fn record_render_state(&mut self, core: &mut TerminalCore) {
        self.previous_cursor = Some((core.get_cursor_row(), core.get_cursor_col()));
        self.previous_selection = self.selection;
        self.previous_blink_visible = self.blink_visible_now(core.get_cursor_blink());
        self.needs_full_redraw = false;
        core.clear_dirty();
    }

    /// Variant for the no-active-tab case (e.g. just after the last tab
    /// exited). Drops the `needs_full_redraw` flag so the next frame can
    /// short-circuit, but leaves cursor/selection history untouched
    /// because no core is available.
    pub fn record_render_state_no_tab(&mut self) {
        self.previous_cursor = None;
        self.previous_selection = None;
        self.previous_blink_visible = true;
        self.needs_full_redraw = false;
    }

    /// Phase 4-E: route an `egui::Event::Ime(ImeEvent::Preedit(_))`
    /// payload to the active tab's preedit state. The anchor is the
    /// current cursor cell of the active tab's `TerminalCore`. No-op
    /// when there is no active tab.
    ///
    /// tao 0.34 only surfaces `WindowEvent::ReceivedImeText` (commit);
    /// this method is the routing point for future preedit plumbing
    /// (richer IME via egui's `ImeEvent::Preedit` once available) and
    /// is exercised directly by the unit tests.
    #[allow(dead_code)]
    pub fn on_ime_preedit(&mut self, text: &str) {
        let Some(tab) = self.tabs.get_mut(self.active) else {
            return;
        };
        let anchor = {
            let core = tab.core.lock();
            crate::ime::preedit::Anchor {
                row: core.get_cursor_row(),
                col: core.get_cursor_col(),
            }
        };
        tab.preedit_state.set(text, anchor);
        // The renderer skips frames when no row is dirty; force the
        // cursor row into the dirty set so the underline overlay
        // repaints immediately.
        self.needs_full_redraw = true;
    }

    /// Phase 4-E: route an `egui::Event::Ime(ImeEvent::Commit(_))`
    /// payload to the active tab. Sanitizes the bytes via
    /// `ime::commit::write_commit` (same sanitizer the preedit state
    /// uses) and writes them to the active PTY exactly once. Then
    /// clears the preedit state so the overlay disappears. No-op when
    /// there is no active tab.
    pub fn on_ime_commit(&mut self, text: &str) {
        let Some(tab) = self.tabs.get_mut(self.active) else {
            return;
        };
        if let Some(pty) = tab.pty.as_ref() {
            if let Err(e) = crate::ime::commit::write_commit(pty, text) {
                log::warn!("ime commit write failed: {e}");
            }
        }
        tab.preedit_state.clear();
        self.needs_full_redraw = true;
    }

    /// Phase 4-E: clear the active tab's preedit state. Called on
    /// focus loss and on active tab close.
    pub fn on_ime_focus_lost(&mut self) {
        if let Some(tab) = self.tabs.get_mut(self.active) {
            tab.preedit_state.clear();
            self.needs_full_redraw = true;
        }
    }

    /// Phase 4-C (APC redesign): route one decoded `MuxMessage` to the
    /// tab at `tab_idx`. The actual routing logic lives on `Tab` so the
    /// tab can mutate its own grid / status state directly — this
    /// wrapper exists primarily as the test seam and as a stable name
    /// for future cross-tab mux behavior.
    ///
    /// Returns `true` when the tab's visible state changed (the caller
    /// should request a redraw).
    pub fn on_mux_message(&mut self, tab_idx: usize, msg: mux_ipc::protocol::MuxMessage) -> bool {
        let Some(tab) = self.tabs.get_mut(tab_idx) else {
            log::warn!("on_mux_message: tab_idx {tab_idx} out of range");
            return false;
        };
        tab.apply_mux_message(msg)
    }
}

impl Default for App {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::selection::{Pos, SelectionMode};
    use term_core::terminal_core::TerminalCore;

    fn fresh_core(cols: u16, rows: u16) -> TerminalCore {
        TerminalCore::new(cols, rows, 100)
    }

    fn app_with_cleared_state(core: &mut TerminalCore) -> App {
        let mut app = App::new();
        // Initial frame uses a full redraw; clear it so subsequent calls
        // exercise the union logic rather than the bypass.
        app.record_render_state(core);
        app
    }

    #[test]
    fn dirty_set_empty_after_clear_when_nothing_moved() {
        let mut core = fresh_core(20, 5);
        let app = app_with_cleared_state(&mut core);
        // Cursor at (0,0), no selection, nothing written.
        let set = app.dirty_rows_this_frame(&core);
        // Current cursor row (0) is always in the set.
        assert_eq!(set, vec![0]);
    }

    #[test]
    fn dirty_set_includes_cursor_move_origin_and_destination() {
        let mut core = fresh_core(20, 5);
        let mut app = app_with_cleared_state(&mut core);
        // Move cursor to row 3 via CSI Cursor Position (1-based).
        core.process_pty_data(b"\x1b[4;1H");
        core.clear_dirty(); // simulate that the write itself didn't touch cells
                            // App still has previous_cursor = (0, 0) from initial record.
        let set = app.dirty_rows_this_frame(&core);
        assert!(
            set.contains(&0),
            "previous cursor row should be in dirty set"
        );
        assert!(
            set.contains(&3),
            "current cursor row should be in dirty set"
        );
        // Record then ask again — cursor history is now (3, x).
        app.record_render_state(&mut core);
        let set2 = app.dirty_rows_this_frame(&core);
        assert_eq!(set2, vec![3]);
    }

    #[test]
    fn dirty_set_includes_selection_extent_rows() {
        let mut core = fresh_core(20, 5);
        let mut app = app_with_cleared_state(&mut core);
        app.selection = Some(Selection {
            anchor: Pos { row: 1, col: 0 },
            extent: Pos { row: 3, col: 0 },
            mode: SelectionMode::Character,
        });
        let set = app.dirty_rows_this_frame(&core);
        // 0 = cursor, 1..=3 = selection.
        assert!(set.contains(&1));
        assert!(set.contains(&2));
        assert!(set.contains(&3));
    }

    #[test]
    fn dirty_set_includes_previous_selection_rows_after_shrink() {
        let mut core = fresh_core(20, 5);
        let mut app = app_with_cleared_state(&mut core);
        // Frame 1: select rows 1..=3.
        app.selection = Some(Selection {
            anchor: Pos { row: 1, col: 0 },
            extent: Pos { row: 3, col: 0 },
            mode: SelectionMode::Character,
        });
        let _ = app.dirty_rows_this_frame(&core);
        app.record_render_state(&mut core);
        // Frame 2: shrink to row 1 only.
        app.selection = Some(Selection {
            anchor: Pos { row: 1, col: 0 },
            extent: Pos { row: 1, col: 0 },
            mode: SelectionMode::Character,
        });
        let set = app.dirty_rows_this_frame(&core);
        assert!(set.contains(&1));
        // Rows 2 and 3 must redraw to clear the old highlight.
        assert!(set.contains(&2), "vacated selection row 2 must redraw");
        assert!(set.contains(&3), "vacated selection row 3 must redraw");
    }

    #[test]
    fn force_full_redraw_returns_all_rows() {
        let mut core = fresh_core(10, 4);
        let mut app = app_with_cleared_state(&mut core);
        app.force_full_redraw = true;
        let set = app.dirty_rows_this_frame(&core);
        assert_eq!(set, vec![0, 1, 2, 3]);
    }

    #[test]
    fn needs_full_redraw_returns_all_rows_until_recorded() {
        let core = fresh_core(10, 3);
        let app = App::new();
        // Default state: needs_full_redraw = true.
        let set = app.dirty_rows_this_frame(&core);
        assert_eq!(set, vec![0, 1, 2]);
    }

    #[test]
    fn dirty_set_after_single_line_write() {
        let mut core = fresh_core(20, 5);
        let app = app_with_cleared_state(&mut core);
        // Write text to row 0 (cursor was at (0,0)).
        core.process_pty_data(b"hello");
        let set = app.dirty_rows_this_frame(&core);
        // Row 0 was edited → in get_dirty_rows. Cursor stayed on row 0.
        assert_eq!(set, vec![0]);
        assert!(
            set.len() < core.rows() as usize,
            "should not be full redraw"
        );
    }

    // ── Scrollback state machine ─────────────────────

    #[test]
    fn scroll_position_default_is_live() {
        let app = App::new();
        assert_eq!(app.scroll_position, ScrollPosition::Live);
        assert_eq!(app.scroll_offset(), 0);
    }

    #[test]
    fn scroll_up_by_advances_offset() {
        let mut app = App::new();
        app.scroll_up_by(3);
        assert_eq!(app.scroll_position, ScrollPosition::OffsetFromLive(3));
        app.scroll_up_by(2);
        assert_eq!(app.scroll_position, ScrollPosition::OffsetFromLive(5));
    }

    #[test]
    fn scroll_up_by_clamps_to_scrollback_lines() {
        let mut app = App::new();
        // Default scrollback_lines = 10_000.
        app.scroll_up_by(99_999);
        assert_eq!(app.scroll_position, ScrollPosition::OffsetFromLive(10_000));
    }

    #[test]
    fn scroll_down_to_zero_snaps_to_live() {
        let mut app = App::new();
        app.scroll_up_by(5);
        app.scroll_down_by(5);
        assert_eq!(app.scroll_position, ScrollPosition::Live);
    }

    #[test]
    fn scroll_down_below_zero_saturates_at_live() {
        let mut app = App::new();
        app.scroll_up_by(3);
        app.scroll_down_by(99);
        assert_eq!(app.scroll_position, ScrollPosition::Live);
    }

    #[test]
    fn scroll_to_top_uses_scrollback_ceiling() {
        let mut app = App::new();
        app.scroll_to_top();
        assert_eq!(app.scroll_position, ScrollPosition::OffsetFromLive(10_000));
    }

    #[test]
    fn scroll_to_live_clears_offset() {
        let mut app = App::new();
        app.scroll_up_by(7);
        app.scroll_to_live();
        assert_eq!(app.scroll_position, ScrollPosition::Live);
    }

    #[test]
    fn alt_screen_suppresses_scroll_up() {
        let mut app = App::new();
        app.alt_screen = true;
        app.scroll_up_by(5);
        assert_eq!(app.scroll_position, ScrollPosition::Live);
    }

    #[test]
    fn alt_screen_suppresses_scroll_to_top() {
        let mut app = App::new();
        app.alt_screen = true;
        app.scroll_to_top();
        assert_eq!(app.scroll_position, ScrollPosition::Live);
    }

    #[test]
    fn set_alt_screen_true_forces_live() {
        let mut app = App::new();
        app.scroll_up_by(5);
        assert_eq!(app.scroll_position, ScrollPosition::OffsetFromLive(5));
        app.set_alt_screen(true);
        assert_eq!(app.scroll_position, ScrollPosition::Live);
        assert!(app.alt_screen);
    }

    #[test]
    fn set_alt_screen_false_preserves_live() {
        let mut app = App::new();
        app.set_alt_screen(true);
        app.set_alt_screen(false);
        assert!(!app.alt_screen);
        assert_eq!(app.scroll_position, ScrollPosition::Live);
    }

    #[test]
    fn on_pty_output_in_live_is_noop() {
        let mut app = App::new();
        app.needs_full_redraw = false;
        app.on_pty_output();
        // No offset change.
        assert_eq!(app.scroll_position, ScrollPosition::Live);
        // No redraw forced (already at live, nothing visual shifted).
        assert!(!app.needs_full_redraw);
    }

    #[test]
    fn on_pty_output_preserves_offset() {
        let mut app = App::new();
        app.scroll_up_by(4);
        app.needs_full_redraw = false;
        app.on_pty_output();
        // Offset preserved: user is not pulled to the bottom.
        assert_eq!(app.scroll_position, ScrollPosition::OffsetFromLive(4));
        // Viewport content shifted underneath us, so a repaint is needed.
        assert!(app.needs_full_redraw);
    }

    // ── Phase 4-B: tab event + AppAction routing ─────────────

    /// TS-tab-2 (App half): closing the last tab empties the tabs
    /// vector. The run loop in `window_host` translates an empty
    /// `app.tabs` into `ControlFlow::Exit` (see `run` in
    /// `window_host.rs`), so this is the `ExitWindow` signal.
    #[test]
    fn closing_last_tab_signals_exit_window() {
        let mut app = App::new();
        // Manually push a Tab-like value would require a PTY; instead
        // we exercise the `close_tab` path on a synthetic tabs vector.
        // Tab::spawn_shell is fine in tests — it returns pty=None when
        // spawn fails, but the tab itself is constructed.
        app.spawn_initial_tab();
        assert_eq!(app.tabs.len(), 1, "exactly one tab after init");
        let exit = app.close_tab(0);
        assert!(exit, "closing the last tab must return true");
        assert!(app.tabs.is_empty(), "tabs vector must be empty after close");

        // The same routing via TabEvent must agree.
        let mut app2 = App::new();
        app2.spawn_initial_tab();
        let exit2 = app2.apply_tab_event(crate::ui::TabEvent::Close(0));
        assert!(exit2);
        assert!(app2.tabs.is_empty());
    }

    #[test]
    fn close_tab_in_middle_shifts_active_left_when_needed() {
        let mut app = App::new();
        app.spawn_initial_tab();
        app.spawn_new_tab();
        app.spawn_new_tab();
        assert_eq!(app.tabs.len(), 3);
        app.active = 2;
        // Close idx 0 → active was 2, now should be 1.
        let exit = app.close_tab(0);
        assert!(!exit);
        assert_eq!(app.active, 1);
        assert_eq!(app.tabs.len(), 2);
    }

    #[test]
    fn close_tab_clamps_active_when_closing_the_active_last_one() {
        let mut app = App::new();
        app.spawn_initial_tab();
        app.spawn_new_tab();
        app.active = 1;
        let exit = app.close_tab(1);
        assert!(!exit);
        // Active falls back to the new last tab.
        assert_eq!(app.active, 0);
        assert_eq!(app.tabs.len(), 1);
    }

    #[test]
    fn next_tab_wraps_at_end() {
        let mut app = App::new();
        app.spawn_initial_tab();
        app.spawn_new_tab();
        app.active = 1;
        let exit = app.apply_action(crate::ui::AppAction::NextTab);
        assert!(!exit);
        assert_eq!(app.active, 0);
    }

    #[test]
    fn prev_tab_wraps_at_start() {
        let mut app = App::new();
        app.spawn_initial_tab();
        app.spawn_new_tab();
        app.active = 0;
        let exit = app.apply_action(crate::ui::AppAction::PrevTab);
        assert!(!exit);
        assert_eq!(app.active, 1);
    }

    #[test]
    fn jump_tab_clamps_to_existing_range() {
        let mut app = App::new();
        app.spawn_initial_tab();
        app.spawn_new_tab();
        // Only two tabs; Ctrl+9 should clamp to the last (idx 1).
        let exit = app.apply_action(crate::ui::AppAction::JumpTab(9));
        assert!(!exit);
        assert_eq!(app.active, 1);
        // Ctrl+1 jumps to idx 0.
        app.apply_action(crate::ui::AppAction::JumpTab(1));
        assert_eq!(app.active, 0);
    }

    #[test]
    fn new_tab_action_appends_and_switches() {
        let mut app = App::new();
        app.spawn_initial_tab();
        let before = app.tabs.len();
        let exit = app.apply_action(crate::ui::AppAction::NewTab);
        assert!(!exit);
        assert_eq!(app.tabs.len(), before + 1);
        assert_eq!(app.active, app.tabs.len() - 1);
    }

    #[test]
    fn close_tab_action_can_signal_exit() {
        let mut app = App::new();
        app.spawn_initial_tab();
        let exit = app.apply_action(crate::ui::AppAction::CloseTab);
        assert!(exit);
    }

    #[test]
    fn tab_event_switch_changes_active_without_exit() {
        let mut app = App::new();
        app.spawn_initial_tab();
        app.spawn_new_tab();
        app.active = 0;
        let exit = app.apply_tab_event(crate::ui::TabEvent::Switch(1));
        assert!(!exit);
        assert_eq!(app.active, 1);
    }

    // ── Phase 4-E: IME preedit/commit routing ────────────────────────

    #[test]
    fn ime_preedit_no_active_tab_is_noop() {
        let mut app = App::new();
        // No spawn → no tabs. Must not panic.
        app.on_ime_preedit("abc");
    }

    #[test]
    fn ime_preedit_updates_active_tab_state() {
        let mut app = App::new();
        app.spawn_initial_tab();
        app.on_ime_preedit("hi");
        let tab = app.active_tab().unwrap();
        assert!(tab.preedit_state.active());
        assert_eq!(tab.preedit_state.text(), "hi");
    }

    #[test]
    fn ime_preedit_anchors_to_current_cursor() {
        let mut app = App::new();
        app.spawn_initial_tab();
        // Move cursor to (row=2, col=3) via CSI CUP.
        {
            let tab = app.active_tab().unwrap();
            tab.core.lock().process_pty_data(b"\x1b[3;4H");
        }
        app.on_ime_preedit("xy");
        let tab = app.active_tab().unwrap();
        let a = tab.preedit_state.anchor();
        assert_eq!(a.row, 2);
        assert_eq!(a.col, 3);
    }

    #[test]
    fn ime_preedit_sanitizes_control_bytes() {
        let mut app = App::new();
        app.spawn_initial_tab();
        // ESC must NOT survive into the preedit overlay text.
        app.on_ime_preedit("a\x1bb");
        assert_eq!(app.active_tab().unwrap().preedit_state.text(), "ab");
    }

    #[test]
    fn ime_commit_clears_preedit_state() {
        let mut app = App::new();
        app.spawn_initial_tab();
        app.on_ime_preedit("abc");
        assert!(app.active_tab().unwrap().preedit_state.active());
        app.on_ime_commit("abc");
        assert!(!app.active_tab().unwrap().preedit_state.active());
    }

    #[test]
    fn ime_commit_no_active_tab_is_noop() {
        let mut app = App::new();
        app.on_ime_commit("abc");
    }

    #[test]
    fn ime_focus_lost_clears_preedit() {
        let mut app = App::new();
        app.spawn_initial_tab();
        app.on_ime_preedit("xy");
        assert!(app.active_tab().unwrap().preedit_state.active());
        app.on_ime_focus_lost();
        assert!(!app.active_tab().unwrap().preedit_state.active());
    }

    #[test]
    fn ime_preedit_requests_full_redraw() {
        let mut app = App::new();
        app.spawn_initial_tab();
        // Clear the initial full-redraw flag so we can observe the
        // routing-time mutation.
        {
            let arc = app.active_tab().unwrap().core.clone();
            let mut core = arc.lock();
            app.record_render_state(&mut core);
        }
        assert!(!app.needs_full_redraw);
        app.on_ime_preedit("ab");
        assert!(app.needs_full_redraw);
    }

    #[test]
    fn ime_commit_requests_full_redraw() {
        let mut app = App::new();
        app.spawn_initial_tab();
        {
            let arc = app.active_tab().unwrap().core.clone();
            let mut core = arc.lock();
            app.record_render_state(&mut core);
        }
        assert!(!app.needs_full_redraw);
        app.on_ime_commit("a");
        assert!(app.needs_full_redraw);
    }

    // ── Phase 4-C (APC redesign): mux message routing ────────────────

    /// TS-mux-msg-1: `App::on_mux_message` applies a `Snapshot` to the
    /// target tab's `TerminalCore` via `reset_and_replay`. The grid
    /// content visible afterward must reflect the replayed bytes.
    #[test]
    fn on_mux_message_snapshot_resets_and_replays_into_core() {
        use mux_ipc::protocol::{MessageType, MuxMessage};

        let mut app = App::new();
        app.spawn_initial_tab();

        // Prime the grid with something the snapshot must overwrite.
        {
            let tab = app.active_tab().unwrap();
            tab.core.lock().process_pty_data(b"BEFORE");
        }

        // Snapshot payload: clear + print "AFTER" at home.
        let mut payload: Vec<u8> = Vec::new();
        payload.extend_from_slice(b"\x1b[2J\x1b[H");
        payload.extend_from_slice(b"AFTER");

        let msg = MuxMessage {
            msg_type: MessageType::Snapshot,
            pane_id: 0,
            payload,
        };
        let changed = app.on_mux_message(0, msg);
        assert!(changed, "Snapshot must mark state changed");

        // The first 5 cells of row 0 should now spell A F T E R.
        let tab = app.active_tab().unwrap();
        let core = tab.core.lock();
        let row0: String = (0..5).map(|c| core.get_cell_char(c, 0)).collect();
        assert_eq!(row0, "AFTER");
    }

    /// TS-mux-msg-2: `App::on_mux_message` updates `mux_status_state`
    /// on the target tab when handed a `StatusUpdate`.
    #[test]
    fn on_mux_message_status_update_caches_payload_on_tab() {
        use mux_ipc::protocol::{MessageType, MuxMessage, StatusUpdateMsg};

        let mut app = App::new();
        app.spawn_initial_tab();

        let payload = StatusUpdateMsg {
            left: "[default] *win1 win2".to_string(),
            right: "12:34".to_string(),
        };
        let msg = MuxMessage::control(MessageType::StatusUpdate, 0, &payload);
        let changed = app.on_mux_message(0, msg);
        assert!(changed);

        let tab = app.active_tab().unwrap();
        let cached = tab.mux_status_state.as_ref().expect("status cached");
        assert_eq!(cached.left, payload.left);
        assert_eq!(cached.right, payload.right);
    }

    /// `App::on_mux_message` with an out-of-range tab index is a no-op
    /// (logs a warning) and never panics.
    #[test]
    fn on_mux_message_out_of_range_returns_false() {
        use mux_ipc::protocol::{MessageType, MuxMessage};

        let mut app = App::new();
        // No tabs spawned.
        let msg = MuxMessage {
            msg_type: MessageType::Snapshot,
            pane_id: 0,
            payload: b"hello".to_vec(),
        };
        assert!(!app.on_mux_message(0, msg));
    }
}
