//! Scrollback navigation, search overlay and fold interaction for [`App`].

use std::time::Instant;

use crate::scroll::ScrollPosition;

use super::App;


/// Minimum wall-clock gap between two automatic (per-frame) search
/// re-resolves. A burst of PTY output flags the search document dirty on
/// every chunk; without this throttle [`App::auto_research_if_dirty`] would
/// rebuild the logical-line document and re-run the match on every painted
/// frame. The dirty flag is *preserved* when a re-search is skipped here, so
/// the pending change is reflected on the next frame past the gap.
pub const AUTO_RESEARCH_THROTTLE: std::time::Duration = std::time::Duration::from_millis(150);

/// Whether an automatic re-search may run now, given the instant of the
/// previous one. `None` (no prior auto re-search) always allows. Split out
/// as a pure function so the throttle policy is unit-testable without an
/// `App` or real wall-clock sleeps.
pub fn auto_research_allowed(last: Option<Instant>, now: Instant) -> bool {
    match last {
        None => true,
        Some(prev) => now.duration_since(prev) >= AUTO_RESEARCH_THROTTLE,
    }
}

/// Direction for [`App::jump_to_prompt`]: `Prev` scrolls toward older
/// prompts (up), `Next` toward newer prompts (down).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JumpDirection {
    Prev,
    Next,
}

impl App {
    /// FR4: whether the active tab/window cell should be scrolled into view
    /// this frame. Read by `render::draw_terminal` (immutable `&App`) and
    /// threaded into the tab strip; cleared post-frame by `window_host`.
    pub fn scroll_active_tab_into_view(&self) -> bool {
        self.scroll_active_tab_into_view
    }

    /// FR4: clear the one-shot scroll-into-view signal after the egui pass so
    /// it fires for exactly one frame and never re-fires on an unrelated
    /// repaint. Called from `window_host::render` where `&mut App` is held.
    pub fn clear_scroll_active_tab_into_view(&mut self) {
        self.scroll_active_tab_into_view = false;
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

    /// Recompute the per-frame [`crate::fold::FoldLayout`] for the active
    /// tab. Called once at the top of `WindowHost::render` (before the
    /// immutable-borrow paint passes). Stores `Some(layout)` only when the
    /// active tab has at least one collapsed region — the renderer's
    /// fold-aware paths gate on `fold_layout().is_some()`, mirroring the
    /// WebView's `getCollapsedRegions().length > 0`. Otherwise stores `None`
    /// so the linear (non-folded) draw path runs unchanged.
    ///
    /// The viewport row count + scrollback length are read from the active
    /// tab's `TerminalCore`; the offset from [`Self::scroll_offset`].
    pub fn refresh_fold_layout(&mut self) {
        let scroll_offset = self.scroll_offset();
        // Snapshot the dimensions under the core lock, then build the layout
        // off the tab's `FoldManager` (which needs `&mut` for its collapsed
        // cache) without holding the core lock across the build.
        let dims = self.active_tab().map(|tab| {
            let core = tab.core.lock();
            (core.get_scrollback_length(), core.rows())
        });
        self.fold_layout = match (dims, self.active_tab_mut()) {
            (Some((scrollback_len, rows)), Some(tab)) => {
                if tab.folds.has_collapsed_regions() {
                    Some(tab.folds.build_layout(scrollback_len, rows, scroll_offset))
                } else {
                    None
                }
            }
            _ => None,
        };
    }

    /// The fold layout built for the current frame, if the active tab has
    /// collapsed regions. `None` selects the linear (non-folded) render
    /// path. See [`Self::refresh_fold_layout`].
    pub fn fold_layout(&self) -> Option<&crate::fold::FoldLayout> {
        self.fold_layout.as_ref()
    }

    /// Toggle (or expand) the fold region under a plain left-click at screen
    /// row `display_row` (0-based, counted from the top grid row). Port of
    /// the WebView `FoldHandler.handleFoldClick` (`handlers/fold.ts`); the
    /// caller (`window_host`) is responsible for the WebView's
    /// outer guards that have no equivalent in pure `App` state: a plain
    /// left-click (no Ctrl/Alt/Meta), with no active text selection, that
    /// did not become a drag-select.
    ///
    /// Behavior, mirroring the WebView:
    /// - Clicking a collapsed region's **summary row** expands that region.
    /// - Clicking inside an **expanded** foldable region collapses it. When
    ///   the region's summary sits *above* the current view top
    ///   (`regionDisplayLine < display_start`) the scroll offset is reduced
    ///   by `line_count - 1` (the rows the collapse hides) so the content
    ///   under the pointer stays visually anchored.
    /// - Clicking outside any region (or in scrollback below a region) is a
    ///   no-op.
    ///
    /// Returns `true` when fold state changed (the caller should request a
    /// redraw). `display_row >= rows` (a click below the grid) is rejected,
    /// matching the WebView's `displayRow < 0 || >= rows` guard.
    pub fn handle_fold_click(&mut self, display_row: u16) -> bool {
        // Snapshot the geometry under the core lock, then operate on the
        // fold manager (which needs `&mut` for its collapsed cache) without
        // holding the lock across the mutation — same pattern as
        // `refresh_fold_layout`.
        let scroll_offset = self.scroll_offset();
        let Some((scrollback_len, rows)) = self.active_tab().map(|tab| {
            let core = tab.core.lock();
            (core.get_scrollback_length(), core.rows())
        }) else {
            return false;
        };
        if display_row >= rows {
            return false;
        }
        let Some(tab) = self.active_tab_mut() else {
            return false;
        };
        if !tab.folds.is_enabled() {
            return false;
        }

        let total_actual = scrollback_len + rows as u32;
        let total_display = tab.folds.get_total_display_lines(total_actual);
        // display_start = max(0, total_display - rows - scroll_offset),
        // saturating to stay in u32 (matches `build_layout`).
        let display_start = total_display
            .saturating_sub(rows as u32)
            .saturating_sub(scroll_offset);
        let display_line = display_start + display_row as u32;

        // A click on a collapsed region's summary row expands it.
        if let Some(region) = tab.folds.get_summary_region(display_line) {
            tab.folds.expand_region_containing(region.start_line);
            self.needs_full_redraw = true;
            return true;
        }

        // Otherwise: a click inside an expanded foldable region collapses it.
        let actual_line = tab.folds.display_line_to_actual(display_line);
        let region = match tab.folds.get_region_at_line(actual_line) {
            Some(r) if !r.collapsed => (r.start_line, r.line_count),
            _ => return false,
        };
        let (region_start, line_count) = region;
        // Capture the region's display row BEFORE collapsing (matches the
        // WebView's `regionDisplayLine` computed before `toggleFold`).
        let region_display_line = tab.folds.actual_line_to_display(region_start);
        tab.folds.toggle_fold(actual_line);
        // Collapsing a region whose summary is above the view top hides
        // `line_count - 1` rows from the scrollback above us; pull the
        // offset down by the same amount so the click target stays put.
        if region_display_line < display_start {
            let delta = line_count - 1;
            let new_offset = scroll_offset.saturating_sub(delta);
            // Re-borrow `self` (the `tab` borrow ends here): `scroll_set_offset`
            // takes `&mut self` and clamps / snaps to Live at 0.
            self.scroll_set_offset(new_offset);
        }
        self.needs_full_redraw = true;
        true
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

    /// Jump to an absolute scrollback offset in rows back from live
    /// (`0` snaps to `Live`). Used by the scrollbar thumb, which
    /// computes its target against the *actual* scrollback length;
    /// the `scrollback_lines` clamp here is a safety net only. No-op
    /// when alt-screen is active.
    pub fn scroll_set_offset(&mut self, offset: u32) {
        if self.alt_screen {
            return;
        }
        self.scroll_position = if offset == 0 {
            ScrollPosition::Live
        } else {
            ScrollPosition::OffsetFromLive(offset.min(self.settings.scrollback_lines))
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

    // ── Prompt-to-prompt navigation (OSC 133) ────────────────

    /// Scroll to the nearest OSC 133 prompt mark in `direction` relative to
    /// the current view top. Port of the WebView
    /// `KeyboardHandler.handlePromptJump` (`handlers/keyboard.ts`).
    ///
    /// Coordinate model: marks carry an absolute scrollback-frame row
    /// (`0..scrollback_len` is scrollback, `scrollback_len..+rows` is the
    /// viewport). The current view top is `scrollback_len - scroll_offset`
    /// (offset is rows back from live). Scrolling to a mark sets the offset
    /// to `scrollback_len - mark_row`, putting the mark line at the top of
    /// the view — a viewport-row mark therefore resolves to a `Live`
    /// (`offset == 0`) position. When no mark exists in `direction`, fall
    /// to the top (`Prev`) or the live tail (`Next`), matching WebView.
    ///
    /// No-op on the alternate screen (same guard as the `scroll_*` family;
    /// alt-screen apps own the whole viewport and have no scrollback).
    ///
    /// Note: when the target prompt mark sits inside a *collapsed* fold
    /// region the region is auto-expanded first (mirroring the WebView
    /// `handlePromptJump`'s `foldManager.expandRegionContaining(marker.lineIndex)`),
    /// so the prompt is visible after the jump. The scroll offset is still
    /// computed against the mark's absolute row — expansion only changes the
    /// display↔actual mapping, not the buffer row the mark lives on, so the
    /// `scrollback_len - row` offset places the prompt at the view top either
    /// way.
    pub fn jump_to_prompt(&mut self, direction: JumpDirection) {
        if self.alt_screen {
            return;
        }
        let scrollback_len = match self.tabs.get(self.active) {
            Some(tab) => tab.core.lock().get_scrollback_length(),
            None => return,
        };
        let current_top_line = scrollback_len.saturating_sub(self.scroll_offset());
        let target = match self.tabs.get(self.active) {
            Some(tab) => match direction {
                JumpDirection::Prev => tab.prompts.find_prev_prompt(current_top_line),
                JumpDirection::Next => tab.prompts.find_next_prompt(current_top_line),
            },
            None => return,
        };
        match target {
            Some(row) => {
                // Auto-expand a collapsed fold region containing the mark so
                // jumping into folded output reveals the prompt. No-op when
                // the mark is in no region (or an already-expanded one).
                if let Some(tab) = self.tabs.get_mut(self.active) {
                    tab.folds.expand_region_containing(row);
                }
                let offset = scrollback_len.saturating_sub(row);
                self.scroll_set_offset(offset);
            }
            None => match direction {
                // No previous prompt: jump to the top of scrollback
                // (`scroll_set_offset` clamps to the configured ceiling).
                JumpDirection::Prev => self.scroll_set_offset(scrollback_len),
                // No next prompt: snap back to the live tail.
                JumpDirection::Next => self.scroll_set_offset(0),
            },
        }
    }

    // ── In-terminal search ───────────────────────────────────

    /// Open the search overlay (or re-focus it if already open). Sets the
    /// one-shot `search_focus_request` so the renderer grabs keyboard
    /// focus + selects the existing query on the next frame, mirroring
    /// `SearchHandler.toggleSearch` → `SearchBar.show()`.
    pub fn open_search(&mut self) {
        self.search.open();
        self.search_focus_request = true;
        self.needs_full_redraw = true;
    }

    /// Close the search overlay and clear its state + highlights.
    pub fn close_search(&mut self) {
        self.search.close();
        self.search_focus_request = false;
        self.needs_full_redraw = true;
    }

    /// Whether the search overlay is currently visible. Used by the key
    /// router in `window_host` to decide between the search-input path
    /// and the normal PTY/keybind path.
    pub fn search_visible(&self) -> bool {
        self.search.visible
    }

    /// Re-run the search against the active tab and update highlights.
    /// Called after the query / options change (incremental search). The
    /// current match is scrolled into view via [`Self::scroll_to_current_match`].
    pub fn run_search(&mut self) {
        if let Some(tab) = self.tabs.get(self.active) {
            let core = tab.core.lock();
            self.search.execute(&core);
        }
        self.scroll_to_current_match();
        self.needs_full_redraw = true;
    }

    /// Re-resolve the matches against the active tab's *current* buffer
    /// without scrolling. Called once per frame (after the pumps in the
    /// event loop) when [`SearchState::needs_research`] is set, so an open
    /// overlay's highlights keep tracking their text as new PTY output (or a
    /// resize) shifts rows into scrollback and changes their absolute row
    /// numbers.
    ///
    /// Time-throttled to one re-resolve per [`AUTO_RESEARCH_THROTTLE`]: a
    /// burst of PTY output flags the document dirty every frame, but
    /// rebuilding the logical-line document + recompiling the match that
    /// often is wasteful. When the gate blocks a run, the dirty flag is
    /// **kept** (we do not call `execute`), so the pending change is picked
    /// up on the next frame past the gap. User-driven [`Self::run_search`]
    /// bypasses the gate.
    ///
    /// Unlike [`Self::run_search`], this deliberately does **not** call
    /// [`Self::scroll_to_current_match`]: an automatic re-resolve must not
    /// yank the viewport, and it preserves the navigation cursor (clamped to
    /// the new match count) rather than snapping back to the first hit, so
    /// the N/M indicator does not jitter as the buffer scrolls (H5). Returns
    /// `true` when a re-search ran so the caller can request a repaint.
    pub fn auto_research_if_dirty(&mut self) -> bool {
        if !self.search.needs_research() {
            return false;
        }
        let now = Instant::now();
        if !auto_research_allowed(self.last_auto_research, now) {
            // Throttled: leave the dirty flag set so the next frame past the
            // gap re-resolves. No `execute`, no repaint request.
            return false;
        }
        self.last_auto_research = Some(now);
        if let Some(tab) = self.tabs.get(self.active) {
            let core = tab.core.lock();
            self.search.execute_preserving_current(&core);
        }
        self.needs_full_redraw = true;
        true
    }

    /// Advance to the next match (wrap-around) and scroll it into view.
    pub fn search_next(&mut self) {
        self.search.next_match();
        self.scroll_to_current_match();
        self.needs_full_redraw = true;
    }

    /// Step to the previous match (wrap-around) and scroll it into view.
    pub fn search_prev(&mut self) {
        self.search.prev_match();
        self.scroll_to_current_match();
        self.needs_full_redraw = true;
    }

    /// Scroll the viewport so the current match is roughly centered, if it
    /// is not already visible. No-op on the alt screen (no scrollback) or
    /// when there is no current match.
    ///
    /// Also auto-expands any collapsed fold region that contains the match,
    /// mirroring the WebView's `search.ts:154`
    /// `foldManager.expandRegionContaining(match.lineIndex)`. The expand
    /// happens before the scroll calculation so the geometry is already
    /// updated when the offset is computed.
    fn scroll_to_current_match(&mut self) {
        if self.alt_screen {
            return;
        }
        // Collect the match row first, then drop the borrow on `self.search`
        // so `self.tabs` can be mutably borrowed for the fold expand below.
        let abs_row = {
            let Some(m) = self.search.current_match() else {
                return;
            };
            // The match's first segment's absolute row anchors the scroll.
            let Some(seg) = m.segments.first() else {
                return;
            };
            seg.abs_row
        };
        // Auto-expand a collapsed fold region containing the match so the
        // hit is visible — mirroring the WebView's search.ts:154 call to
        // `foldManager.expandRegionContaining(match.lineIndex)`.
        // No-op when the match is in no region or an already-expanded one.
        if let Some(tab) = self.tabs.get_mut(self.active) {
            tab.folds.expand_region_containing(abs_row);
        }
        let (scrollback_len, rows) = match self.tabs.get(self.active) {
            Some(tab) => {
                let core = tab.core.lock();
                (core.get_scrollback_length(), core.rows())
            }
            None => return,
        };
        if let Some(offset) = crate::search::scroll_offset_for_match(
            abs_row,
            scrollback_len,
            rows,
            self.scroll_offset(),
        ) {
            self.scroll_set_offset(offset);
        }
    }
}
