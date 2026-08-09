//! Tab lifecycle (spawn / close / switch / reorder / reconcile) for [`App`].

use crate::tabs::Tab;

use super::agent_status::{agent_notification_rate_limit_key, agent_status_keys_for_tab};
use super::{App, MuxActionOutcome};

impl App {
    /// Spawn the initial shell tab. Called once at startup.
    pub fn spawn_initial_tab(&mut self) {
        let dims = self.cell_size;
        let tab = Tab::spawn_shell(
            "shell",
            dims.cols,
            dims.rows,
            self.settings.scrollback_lines,
            self.settings.clone(),
            Some(self.status_bar_runtime.dispatcher()),
            Some(self.status_bar_runtime.cwd_provider()),
            self.notification_sink.clone(),
            None,
        );
        self.tabs.push(tab);
        self.active = 0;
        // A brand-new tab populated rows; ensure the first frame draws them.
        self.needs_full_redraw = true;
    }

    /// Spawn an additional shell tab using the global settings, switch to
    /// it, and request a repaint. Used by `AppAction::NewTabGlobal` and as
    /// the no-default-profile fallback of [`App::spawn_new_tab_profile_aware`].
    pub fn spawn_new_tab(&mut self) {
        self.spawn_new_tab_with_overrides(None);
    }

    /// Spawn an additional shell tab with optional profile spawn
    /// overrides, switch to it, and request a repaint.
    pub fn spawn_new_tab_with_overrides(
        &mut self,
        overrides: Option<crate::profiles::SpawnOverrides>,
    ) {
        let dims = self.cell_size;
        let tab = Tab::spawn_shell(
            "shell",
            dims.cols,
            dims.rows,
            self.settings.scrollback_lines,
            self.settings.clone(),
            Some(self.status_bar_runtime.dispatcher()),
            Some(self.status_bar_runtime.cwd_provider()),
            self.notification_sink.clone(),
            overrides,
        );
        // A fresh tab seeds its theme from `settings.font_size`; carry
        // the live zoom level over so a tab opened after the user zoomed
        // matches the existing tabs instead of snapping back to the
        // configured baseline.
        if (self.runtime_font_size_pt - self.settings.font_size).abs() >= f32::EPSILON {
            tab.theme.lock().font_size_pt = self.runtime_font_size_pt;
        }
        self.tabs.push(tab);
        self.active = self.tabs.len() - 1;
        // FR4-adjacent: a freshly created tab lands at the end of the strip,
        // which is off-screen when the tabs overflow. Raise the same one-shot
        // scroll-into-view flag the keyboard switch path uses so the new tab
        // surfaces next frame. Unlike an existing-tab mouse click (already
        // visible, so it must NOT scroll — see `switch_to_tab`), a tab the user
        // has not seen yet should always surface, whether opened via the `+`
        // button or a keybind. All new-tab paths funnel through here.
        self.scroll_active_tab_into_view = true;
        self.needs_full_redraw = true;
    }

    /// `new_tab` keybind: apply the `is_default` profile when one exists,
    /// otherwise spawn with the global settings. Port of the WebView's
    /// `keyboard-handler.ts::handleNewTab`. A profile that fails to
    /// resolve (unknown SSH connection / unconfigured ssh path) logs an
    /// error and spawns nothing, matching the WebView's alert-and-abort.
    pub fn spawn_new_tab_profile_aware(&mut self) {
        let Some(profile) = crate::profiles::default_profile(&self.settings.profiles) else {
            self.spawn_new_tab();
            return;
        };
        match crate::profiles::resolve_spawn(profile, &self.settings) {
            Ok(overrides) => self.spawn_new_tab_with_overrides(Some(overrides)),
            Err(e) => log::error!("profile {:?}: {e}", profile.name),
        }
    }

    /// Close the tab at `idx`. Returns `true` when the close emptied
    /// the tabs vector, signaling the app loop that the window should
    /// exit. `tabs.is_empty()` is the same signal; this is a
    /// convenience for code that needs to branch immediately after
    /// the close.
    /// Request closing the tab at `idx`. When that tab has active SFTP
    /// uploads, the close is deferred behind a confirmation dialog (the
    /// `close_guard` is armed and the tab stays open) and `false` is returned.
    /// Otherwise it closes immediately via [`Self::close_tab`].
    pub fn request_close_tab(&mut self, idx: usize) -> bool {
        if let Some(tab) = self.tabs.get(idx) {
            if self.sftp_service.has_active_for_tab(tab.stable_id) {
                // Store the tab's stable_id, not its index: the roster can
                // change (tabs added/removed/reordered) while the guard dialog
                // is open, which would invalidate a stored index (#7).
                self.sftp_ui.close_guard = Some(tab.stable_id);
                return false;
            }
        }
        self.close_tab(idx)
    }

    /// Confirm a guarded tab close: cancel the guarded tab's active uploads,
    /// then close it. The guard holds a stable_id, resolved to a current index
    /// here; if the tab is already gone the guard is cleared cleanly.
    /// Returns true when the close emptied the tabs vector.
    pub fn confirm_close_guard(&mut self) -> bool {
        let Some(id) = self.sftp_ui.close_guard.take() else {
            return false;
        };
        let Some(idx) = self.tabs.iter().position(|t| t.stable_id == id) else {
            // The guarded tab no longer exists; nothing to close.
            return self.tabs.is_empty();
        };
        // Cancel only the guarded tab's sessions.
        self.sftp_service.cancel_for_tab(id);
        self.close_tab(idx)
    }

    /// Dismiss the close-guard dialog without closing the tab.
    pub fn cancel_close_guard(&mut self) {
        self.sftp_ui.close_guard = None;
    }

    pub fn close_tab(&mut self, idx: usize) -> bool {
        if idx >= self.tabs.len() {
            return self.tabs.is_empty();
        }
        // task0005 AC-6 / task0009 AC-4: discard this tab's agent-status
        // entries AND its notification rate-limit bookkeeping before
        // removal (its own plain-tab key plus every mux pane in its window
        // group, if attached).
        for key in agent_status_keys_for_tab(&self.tabs[idx]) {
            let rate_limit_key = agent_notification_rate_limit_key(&self.mux_public_pane_ids, &key);
            self.discard_agent_notification_state(&rate_limit_key);
            self.agent_status.discard(&key);
        }
        // Drop the tab — its `PtySession::Drop` impl kills the child
        // and joins reader/writer threads.
        self.tabs.remove(idx);
        // Closing a tab shifts the active buffer; any open search overlay
        // is now indexed into a buffer that may no longer be active, so
        // clear it (mirrors the switch_to_tab rationale).
        if self.search.visible {
            self.search.close();
        }
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
        // D2: the active index may have just moved onto a different tab
        // (the closed tab was the active one, or the last one); request a
        // reconcile of its size against `cell_size` (see
        // `request_active_tab_reconcile` for why this is deferred rather
        // than executed here). No-op when the shift only re-indexed the
        // SAME tab (still at `cell_size` already) or landed on a tab
        // already at size.
        self.request_active_tab_reconcile();
        false
    }

    /// Switch to the tab at `idx` (no-op for out-of-range / same idx).
    pub fn switch_to_tab(&mut self, idx: usize) {
        if idx >= self.tabs.len() || idx == self.active {
            return;
        }
        // FR3: park the outgoing tab's live scroll position into its own slot,
        // then commit the new active index, then reload the incoming tab's
        // saved position into the single active value (`scroll_position`) the
        // renderer and the scroll mutators read. `Live` restores to the
        // bottom; `OffsetFromLive(n)` restores to that offset. The scroll-pin
        // hot path (wheel / PageUp) is untouched — it still reads/writes the
        // single `scroll_position`; only the switch boundary swaps it.
        if let Some(outgoing) = self.tabs.get_mut(self.active) {
            outgoing.scroll_position = self.scroll_position;
        }
        self.active = idx;
        // FR4: the scroll-into-view flag is intentionally NOT raised here.
        // `switch_to_tab` is also reached by mouse paths (`TabEvent::Switch`
        // and `MuxSwitch`), and FR4 fires only on keyboard activation. The
        // keyboard handlers (`NextTab` / `PrevTab` / `JumpTab`) raise the flag
        // themselves after confirming the active index actually moved.
        if let Some(incoming) = self.tabs.get(self.active) {
            self.scroll_position = incoming.scroll_position;
        }
        // The search overlay's matches are indexed into the previous
        // tab's scrollback + viewport, so they are meaningless against
        // the new tab. Close the overlay + clear state on every active-
        // tab change (the WebView keeps a single SearchHandler with no
        // per-tab restore hook in the basic build, so closing is the
        // faithful, non-stale behavior).
        if self.search.visible {
            self.search.close();
        }
        // The selection holds absolute-row buffer coordinates of the
        // *previous* tab, which are meaningless against the new tab's
        // buffer (same rationale as closing the search overlay above), so
        // drop it on every active-tab change.
        self.selection = None;
        // The pending press anchor is likewise scoped to the previous tab's
        // buffer, so drop it as well.
        self.pending_selection_anchor = None;
        self.needs_full_redraw = true;
        // D2: the incoming tab just became the committed active tab;
        // request that its size be brought in line with the current
        // display area — deferred to `execute_pending_reconcile` rather
        // than run here (see that method's doc for why: at this point in
        // the frame `cell_size` may still describe the OUTGOING tab's
        // display area).
        self.request_active_tab_reconcile();
    }

    /// D2 (FR3, task0003 rework): record that the active tab may need
    /// reconciling against `self.cell_size`. Deliberately does NOT compare
    /// or resize here — at the moment every activation path (explicit
    /// switch, close-tab fix-up, exited-tab reap fix-up) calls this,
    /// `self.cell_size` can still hold dims computed for the OUTGOING tab:
    /// the persistent mux sidebar's width inset depends on whether the
    /// ACTIVE tab is mux-attached (`App::mux_sidebar_visibility`), so a
    /// synchronous compare-and-resize here would size the incoming tab
    /// against the wrong display area (round-1 findings dbb7766a6212fb1a /
    /// 09f0e6096bbc36ee). The actual comparison happens in
    /// [`Self::execute_pending_reconcile`], invoked by `window_host::render`
    /// once insets for the NEW active tab have settled. Consecutive
    /// requests before that point collapse into one.
    pub(super) fn request_active_tab_reconcile(&mut self) {
        self.pending_active_tab_reconcile = true;
    }

    /// D2 (FR3, task0003 rework): consume a pending activation-reconcile
    /// request (if any). Called by `window_host::render` at exactly one
    /// point — after the status-bar insets, the mux-sidebar inset, and any
    /// pending display-area resize have settled `self.cell_size` for the
    /// NEW active tab — so the comparison below is against the INCOMING
    /// tab's own settled display area, never the outgoing tab's stale one.
    /// On a mismatch, resizes that tab (and ONLY that tab) through the App
    /// resize application path ([`Self::apply_tab_resize`], FR6); on a
    /// match, issues nothing and clears nothing (FR3's no-op guarantee) —
    /// most calls are no-ops, since a tab's size only ever drifts from
    /// `cell_size` while it sits inactive. A request with no matching
    /// active tab (e.g. all tabs closed) is dropped silently.
    pub fn execute_pending_reconcile(&mut self) {
        if !std::mem::take(&mut self.pending_active_tab_reconcile) {
            return;
        }
        let Some(tab) = self.tabs.get(self.active) else {
            return;
        };
        let (cols, rows) = {
            let core = tab.core.lock();
            (core.cols(), core.rows())
        };
        if cols != self.cell_size.cols || rows != self.cell_size.rows {
            self.apply_tab_resize(self.active, self.cell_size.cols, self.cell_size.rows);
        }
    }

    /// Design 2 (FR6, task0003 rework): the single App-side path for
    /// issuing a `Tab::resize` on ANY origin — a display-area change
    /// (`set_grid_size`) or an activation reconcile
    /// (`execute_pending_reconcile`). Reads the target tab's pre-resize
    /// column count, applies the resize (`Tab::resize` owns the clamp, the
    /// tab's OWN reflow-invalidated trackers, and mux pane `Resize` frame
    /// emission — contracts (a)-(e), untouched), then — if the resize
    /// changed the tab's (post-clamp) column count — clears the App-OWNED
    /// reflow-invalidated trackers (selection, pending selection anchor;
    /// D3). BOTH callers issue their resize through this path, so the
    /// App-side half of the D3 split holds on every origin regardless of
    /// which activation path (or `set_grid_size`) triggered it — it is
    /// never left to caller preconditions (round-1 findings
    /// a172de726b3cbc29 / d39a6a9468ff892e).
    pub(super) fn apply_tab_resize(&mut self, idx: usize, cols: u16, rows: u16) {
        let Some(tab) = self.tabs.get_mut(idx) else {
            return;
        };
        let pre_resize_cols = tab.core.lock().cols();
        tab.resize(cols, rows);
        let post_resize_cols = tab.core.lock().cols();
        if post_resize_cols != pre_resize_cols {
            self.selection = None;
            self.pending_selection_anchor = None;
        }
    }

    /// Move the tab at `from` to land at position `to` (drag-and-drop
    /// reorder). `to` is the **insertion index** into the post-removal
    /// vector, in 0..=tabs.len(). No-op when:
    ///
    /// - `from` is out of range
    /// - `to` falls on either side of `from` (would land in the same
    ///   slot — `from` itself or immediately after it).
    ///
    /// The active index is fixed up so the same logical tab remains
    /// active after the move.
    pub fn reorder_tab(&mut self, from: usize, to: usize) {
        if from >= self.tabs.len() {
            return;
        }
        // Skip moves that would land the tab in its current slot.
        if to == from || to == from + 1 {
            return;
        }
        let to = to.min(self.tabs.len());
        let tab = self.tabs.remove(from);
        // After `remove`, indices >= `from` shifted down by 1; adjust
        // the insertion point to match the user's intent.
        let insert_at = if to > from { to - 1 } else { to };
        self.tabs.insert(insert_at, tab);

        // Fix up `self.active` so the logically-active tab follows the
        // move. Four cases:
        //   - The moved tab was active → it now lives at `insert_at`.
        //   - Active was to the left of `from` and to the left of
        //     `insert_at` → unchanged.
        //   - Active was to the right of `from` and to the right of
        //     `insert_at` → unchanged.
        //   - Otherwise the active tab shifts by ±1.
        if self.active == from {
            self.active = insert_at;
        } else if from < self.active && insert_at >= self.active {
            self.active -= 1;
        } else if from > self.active && insert_at <= self.active {
            self.active += 1;
        }

        self.needs_full_redraw = true;
    }

    /// Open the settings window (child `--settings` process). Replaces
    /// the former in-app egui settings tab: the WebView settings panel
    /// runs in its own window, and saves are applied live through the
    /// launcher's save-event watcher.
    pub fn open_settings_window(&mut self) {
        self.settings_launcher.open();
    }

    /// Apply a [`crate::ui::TabEvent`] emitted by the tab bar widget.
    /// Returns `true` when the resulting state should exit the window
    /// (i.e. the last tab was closed).
    pub fn apply_tab_event(&mut self, evt: crate::ui::TabEvent) -> bool {
        // Mux rename / move dialogs commit against `self.tabs.get_mut(self.active)`
        // (they only carry a `window_id`, not an owning-tab anchor). Any tab
        // event that mutates `self.tabs` / `self.active` (creation, close,
        // reorder, switch, mux-switch — and any future variant we add) makes
        // the captured anchor stale or out-of-range, so the conservative
        // choice is to drop *every* tab event while a dialog is open. Fail
        // closed: a new `TabEvent` variant added later defaults to "blocked
        // while dialog open", not "silently leaks past the guard".
        if self.mux_dialog_open() {
            return false;
        }
        match evt {
            crate::ui::TabEvent::New => {
                // `+` button: plain spawn without profiles, otherwise the
                // new-tab chooser modal (WebView `handleNewTabClick`).
                self.open_new_tab_chooser();
                false
            }
            crate::ui::TabEvent::OpenSettings => {
                self.open_settings_window();
                false
            }
            crate::ui::TabEvent::Close(idx) => self.request_close_tab(idx),
            crate::ui::TabEvent::Switch(idx) => {
                if idx < self.tabs.len() {
                    self.switch_to_tab(idx);
                }
                false
            }
            crate::ui::TabEvent::Reorder { from, to } => {
                if from >= self.tabs.len() {
                    return false;
                }
                self.reorder_tab(from, to.min(self.tabs.len()));
                false
            }
            crate::ui::TabEvent::MuxSwitch { tab, window } => {
                // Sub-tab click: focus the tab if needed, then switch the
                // mux window (local active index + `SwitchWindow`), mirroring
                // the keyboard `prefix 0..9` path (FR5).
                if tab < self.tabs.len() {
                    if self.active != tab {
                        self.switch_to_tab(tab);
                    }
                    // Swap the active scroll value through the per-pane slots
                    // (FR3) and force a full redraw on a committed switch
                    // (FR2), mirroring the keyboard prefix path.
                    let mut scroll = self.scroll_position;
                    let outcome = match self.tabs.get_mut(self.active) {
                        Some(t) => Self::switch_to(t, Some(window), &mut scroll),
                        None => MuxActionOutcome::None,
                    };
                    if outcome == MuxActionOutcome::Changed {
                        self.scroll_position = scroll;
                        self.needs_full_redraw = true;
                        // task0002 AC-6: the sidebar-row-click switch path
                        // also counts as a switch (SPEC.md A6).
                        self.record_mux_window_switch();
                    }
                }
                false
            }
        }
    }
}
