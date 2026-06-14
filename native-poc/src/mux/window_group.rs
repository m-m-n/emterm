//! Per-tab mux window state model.
//!
//! Port of the `muxWindows` / `muxPaneIds` bookkeeping spread across
//! `src/terminal-app/mux/mux-window-manager.ts`. A mux-attached `Tab` owns
//! one [`MuxWindowGroup`]; the tab bar renders it and the prefix/dialog
//! actions mutate it.
//!
//! ## Invariant F1 (parallel arrays)
//!
//! [`MuxWindowGroup::windows`] and [`MuxWindowGroup::pane_ids`] are
//! **index-aligned parallel collections** (same length, same order). Every
//! mutation (append / remove / reorder) MUST update both together, matching
//! the WebView `muxWindows` / `muxPaneIds` pairing. The mutating methods on
//! this type are the only sanctioned way to touch the lists, so the
//! invariant is enforced in one place.
//!
//! The module is intentionally pure — no egui, no I/O, no protocol concerns.

/// One mux window as tracked by the GUI: a stable daemon window id and the
/// current display name. Mirrors the WebView `{ id, name }` entries in
/// `muxWindows`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MuxWindow {
    /// Daemon-assigned window id. Stable across reorders; the dialog
    /// re-resolution path keys on this (FR7 / FR8 stable-id re-resolve).
    pub id: u32,
    /// Display label shown in the sub-tab.
    pub name: String,
}

/// Per-tab mux window state.
///
/// The WebView always renders an attached mux tab as a group of per-window
/// sub-tabs (no compact/expanded toggle); this type therefore only tracks
/// the window list, active index, and pending-create accounting.
#[derive(Debug, Clone, Default)]
pub struct MuxWindowGroup {
    /// Ordered window list. Parallel to [`Self::pane_ids`] (invariant F1).
    windows: Vec<MuxWindow>,
    /// Ordered pane ids. Parallel to [`Self::windows`] (invariant F1).
    pane_ids: Vec<u32>,
    /// Active window index. Always clamped into `[0, len - 1]` while the
    /// list is non-empty; `0` when empty.
    active: usize,
    /// Count of `CreateWindow` requests sent but not yet matched by a
    /// daemon `PaneCreated`. Port of `muxPendingWindowCount`.
    pending_create: u32,
}

impl MuxWindowGroup {
    /// A fresh, empty group.
    pub fn new() -> Self {
        Self::default()
    }

    // ── queries ─────────────────────────────────────────────────────────

    /// Number of windows in the group.
    pub fn len(&self) -> usize {
        self.windows.len()
    }

    /// Whether the group holds no windows.
    pub fn is_empty(&self) -> bool {
        self.windows.is_empty()
    }

    /// Read access to the window list (parallel to [`Self::pane_ids`]).
    pub fn windows(&self) -> &[MuxWindow] {
        &self.windows
    }

    /// Read access to the pane-id list (parallel to [`Self::windows`]).
    pub fn pane_ids(&self) -> &[u32] {
        &self.pane_ids
    }

    /// Current active window index (`0` when empty).
    pub fn active_index(&self) -> usize {
        self.active
    }

    /// The pane id of the active window, if any.
    pub fn active_pane_id(&self) -> Option<u32> {
        self.pane_ids.get(self.active).copied()
    }

    /// The active window, if any.
    pub fn active_window(&self) -> Option<&MuxWindow> {
        self.windows.get(self.active)
    }

    /// Whether this group should render as a tab group of sub-tabs. Matching
    /// the WebView, an attached mux tab renders sub-tabs whenever it holds at
    /// least one window; the group is dissolved (reverts to a plain tab) only
    /// when it drops to zero windows (FR9, handled by the caller clearing the
    /// `Option`).
    pub fn is_group(&self) -> bool {
        !self.windows.is_empty()
    }

    /// Number of outstanding `CreateWindow` requests.
    pub fn pending_create(&self) -> u32 {
        self.pending_create
    }

    /// A locally-unique window id for a fresh create. The WebView uses the
    /// new list index as the synthetic id; we instead return one past the
    /// current maximum id so a fresh create never collides with a
    /// daemon-seeded window id (which the stable-id re-resolution in the
    /// rename / move dialogs depends on). Returns `0` for an empty group.
    ///
    /// At `u32::MAX` the +1 path is unavailable; falls back to a linear scan
    /// for the smallest unused id so the documented no-collision invariant
    /// still holds. Practically unreachable (4 B windows) but keeps
    /// `index_of_window_id` from silently retargeting on a collision.
    pub fn fresh_window_id(&self) -> u32 {
        let max = self.windows.iter().map(|w| w.id).max();
        match max {
            None => 0,
            Some(m) => m.checked_add(1).unwrap_or_else(|| {
                (0u32..)
                    .find(|id| !self.windows.iter().any(|w| w.id == *id))
                    .expect("with <2^32 windows at least one id must be unused")
            }),
        }
    }

    /// Find the list index of the window with the given stable id. Used by
    /// the rename / move dialogs to re-resolve a captured window after the
    /// async dialog (FR7 / FR8).
    pub fn index_of_window_id(&self, window_id: u32) -> Option<usize> {
        self.windows.iter().position(|w| w.id == window_id)
    }

    /// Find the list index of the window owning the given pane id.
    pub fn index_of_pane_id(&self, pane_id: u32) -> Option<usize> {
        self.pane_ids.iter().position(|&p| p == pane_id)
    }

    // ── mutation (invariant F1: both arrays move together) ──────────────

    /// Seed the window list from a daemon `SessionInfo` (Welcome ingest).
    /// Replaces any current contents. The active index is clamped into
    /// range. Port of the reattach seeding path.
    pub fn seed(&mut self, windows: Vec<MuxWindow>, pane_ids: Vec<u32>, active: usize) {
        debug_assert_eq!(
            windows.len(),
            pane_ids.len(),
            "F1: seed windows/pane_ids length mismatch"
        );
        self.windows = windows;
        self.pane_ids = pane_ids;
        self.set_active_clamped(active);
    }

    /// Append a window/pane pair and make it active. Port of the
    /// `handleMuxPaneCreated` push (initial name "Terminal" decided by the
    /// caller). Returns the new (now active) index.
    pub fn push(&mut self, window: MuxWindow, pane_id: u32) -> usize {
        self.windows.push(window);
        self.pane_ids.push(pane_id);
        let idx = self.windows.len() - 1;
        self.active = idx;
        idx
    }

    /// Remove the window/pane owning `pane_id` (shell exit). Re-clamps the
    /// active index into `[0, len - 1]`. Port of `handleMuxPaneExited`'s
    /// splice + index adjustment. Returns the removed index, or `None` when
    /// the pane is unknown.
    pub fn remove_pane(&mut self, pane_id: u32) -> Option<usize> {
        let idx = self.index_of_pane_id(pane_id)?;
        self.windows.remove(idx);
        self.pane_ids.remove(idx);
        if self.active >= self.windows.len() {
            self.active = self.windows.len().saturating_sub(1);
        }
        Some(idx)
    }

    /// Relabel the window with the given stable id. Port of inbound
    /// RenameWindow / the optimistic rename. Returns true when a window
    /// matched.
    pub fn rename_window_id(&mut self, window_id: u32, name: String) -> bool {
        match self.windows.iter_mut().find(|w| w.id == window_id) {
            Some(w) => {
                w.name = name;
                true
            }
            None => false,
        }
    }

    /// Set the active index, clamped into `[0, len - 1]` (or `0` when empty).
    /// Port of `MuxTabGroup.setActiveWindow` / `updateWindowNames` clamping.
    pub fn set_active_clamped(&mut self, index: usize) {
        if self.windows.is_empty() {
            self.active = 0;
        } else {
            self.active = index.min(self.windows.len() - 1);
        }
    }

    /// Set the active index to the window owning `pane_id` (inbound
    /// SwitchWindow). Returns true when the pane matched a known window.
    pub fn set_active_by_pane(&mut self, pane_id: u32) -> bool {
        match self.index_of_pane_id(pane_id) {
            Some(idx) => {
                self.active = idx;
                true
            }
            None => false,
        }
    }

    /// Compute the next-window index (wrap-around). `None` when fewer than
    /// two windows (switch is a no-op). Port of the `next-window` action.
    pub fn next_index(&self) -> Option<usize> {
        let len = self.windows.len();
        if len < 2 {
            return None;
        }
        Some((self.active + 1) % len)
    }

    /// Compute the previous-window index (wrap-around). `None` when fewer
    /// than two windows. Port of the `prev-window` action.
    pub fn prev_index(&self) -> Option<usize> {
        let len = self.windows.len();
        if len < 2 {
            return None;
        }
        Some((self.active + len - 1) % len)
    }

    /// Resolve a `prefix 0..9` digit jump to a concrete index. The digit is
    /// clamped to the existing window range; `None` when the digit is past
    /// the range entirely (no-op past range, per the SPEC edge case) or
    /// there are no windows.
    pub fn digit_index(&self, digit: u8) -> Option<usize> {
        if self.windows.is_empty() {
            return None;
        }
        let d = digit as usize;
        if d >= self.windows.len() {
            // Past the range — no-op (do not jump to the last window).
            None
        } else {
            Some(d)
        }
    }

    /// Reorder: remove the element at `from` and re-insert it at `to`,
    /// adjusting the active index to follow the movement. Port of
    /// `reorderMuxWindows` (remove-then-insert, matching the daemon's
    /// `MoveWindow`). Returns true iff the order actually changed.
    ///
    /// Both arrays move together (invariant F1).
    pub fn reorder(&mut self, from: usize, to: usize) -> bool {
        let len = self.windows.len();
        if len == 0 || from >= len || to >= len || from == to {
            return false;
        }
        let win = self.windows.remove(from);
        self.windows.insert(to, win);
        let pane = self.pane_ids.remove(from);
        self.pane_ids.insert(to, pane);

        let active = self.active;
        if active == from {
            self.active = to;
        } else if from < active && active <= to {
            self.active = active - 1;
        } else if to <= active && active < from {
            self.active = active + 1;
        }
        true
    }

    /// Increment the pending-create counter (a `CreateWindow` was sent).
    pub fn inc_pending_create(&mut self) {
        self.pending_create += 1;
    }

    /// Consume one pending-create credit when a `PaneCreated` arrives.
    /// Returns true when a credit was available (caller should append the
    /// window). A `PaneCreated` with no pending credit is ignored to avoid
    /// phantom sub-tabs, matching `handleMuxPaneCreated`'s early return.
    pub fn take_pending_create(&mut self) -> bool {
        if self.pending_create > 0 {
            self.pending_create -= 1;
            true
        } else {
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn win(id: u32, name: &str) -> MuxWindow {
        MuxWindow {
            id,
            name: name.to_string(),
        }
    }

    fn group_with(n: usize) -> MuxWindowGroup {
        let mut g = MuxWindowGroup::new();
        let windows: Vec<MuxWindow> = (0..n).map(|i| win(i as u32, &format!("w{i}"))).collect();
        let panes: Vec<u32> = (0..n).map(|i| 100 + i as u32).collect();
        g.seed(windows, panes, 0);
        g
    }

    // ── TS-3: active-index clamp on shrink ───────────────────────────────

    #[test]
    fn remove_reclamps_active_index() {
        let mut g = group_with(3); // active = 0
        g.set_active_clamped(2);
        assert_eq!(g.active_index(), 2);
        // Remove the active (last) window's pane → active clamps to new last.
        g.remove_pane(102);
        assert_eq!(g.len(), 2);
        assert_eq!(g.active_index(), 1);
    }

    #[test]
    fn set_active_clamps_into_range() {
        let mut g = group_with(2);
        g.set_active_clamped(99);
        assert_eq!(g.active_index(), 1);
        let empty = MuxWindowGroup::new();
        assert_eq!(empty.active_index(), 0);
    }

    #[test]
    fn remove_non_active_keeps_active_window() {
        let mut g = group_with(3); // panes 100,101,102; active 0
        g.set_active_clamped(2); // active window id 2 (pane 102)
        let removed = g.remove_pane(100); // remove first
        assert_eq!(removed, Some(0));
        // active followed the shrink: index now 1, still window id 2.
        assert_eq!(g.active_window().unwrap().id, 2);
    }

    // ── seed / push / dissolve ───────────────────────────────────────────

    #[test]
    fn push_appends_and_activates() {
        let mut g = MuxWindowGroup::new();
        let idx = g.push(win(7, "Terminal"), 200);
        assert_eq!(idx, 0);
        assert_eq!(g.active_index(), 0);
        assert_eq!(g.active_pane_id(), Some(200));
        let idx2 = g.push(win(8, "Terminal"), 201);
        assert_eq!(idx2, 1);
        assert_eq!(g.active_index(), 1);
    }

    #[test]
    fn is_group_whenever_non_empty() {
        // WebView parity: an attached mux tab renders sub-tabs even for a
        // single window; only an empty group is not a group.
        assert!(!MuxWindowGroup::new().is_group());
        assert!(group_with(1).is_group());
        assert!(group_with(2).is_group());
    }

    #[test]
    fn parallel_arrays_stay_aligned_through_mutations() {
        let mut g = group_with(3);
        g.push(win(9, "extra"), 109);
        g.remove_pane(101);
        g.reorder(0, 2);
        assert_eq!(g.windows().len(), g.pane_ids().len());
    }

    // ── rename / switch by pane ──────────────────────────────────────────

    #[test]
    fn rename_by_window_id() {
        let mut g = group_with(2);
        assert!(g.rename_window_id(1, "editor".to_string()));
        assert_eq!(g.windows()[1].name, "editor");
        assert!(!g.rename_window_id(999, "nope".to_string()));
    }

    #[test]
    fn set_active_by_pane_syncs_index() {
        let mut g = group_with(3); // panes 100,101,102
        assert!(g.set_active_by_pane(102));
        assert_eq!(g.active_index(), 2);
        assert!(!g.set_active_by_pane(999));
        assert_eq!(g.active_index(), 2); // unchanged on miss
    }

    // ── switch index math (TS-12) ────────────────────────────────────────

    #[test]
    fn next_prev_wrap_around() {
        let mut g = group_with(3);
        assert_eq!(g.next_index(), Some(1));
        g.set_active_clamped(2);
        assert_eq!(g.next_index(), Some(0)); // wrap
        assert_eq!(g.prev_index(), Some(1));
        g.set_active_clamped(0);
        assert_eq!(g.prev_index(), Some(2)); // wrap
    }

    #[test]
    fn single_window_switch_is_noop() {
        let g = group_with(1);
        assert_eq!(g.next_index(), None);
        assert_eq!(g.prev_index(), None);
    }

    #[test]
    fn digit_jump_clamps_and_noops_past_range() {
        let g = group_with(3);
        assert_eq!(g.digit_index(0), Some(0));
        assert_eq!(g.digit_index(2), Some(2));
        assert_eq!(g.digit_index(3), None); // past range → no-op
        assert_eq!(g.digit_index(9), None);
        assert_eq!(MuxWindowGroup::new().digit_index(0), None);
    }

    // ── reorder (TS-13 reorder semantics) ────────────────────────────────

    #[test]
    fn reorder_moves_element_and_follows_active() {
        // windows: 0,1,2,3 ; active = 0
        let mut g = group_with(4);
        // move window 0 to position 2 → order becomes 1,2,0,3
        assert!(g.reorder(0, 2));
        assert_eq!(
            g.windows().iter().map(|w| w.id).collect::<Vec<_>>(),
            vec![1, 2, 0, 3]
        );
        // active followed the moved element to its new index.
        assert_eq!(g.active_index(), 2);
    }

    #[test]
    fn reorder_shifts_active_when_crossed() {
        let mut g = group_with(4);
        g.set_active_clamped(1); // active window id 1
                                 // move window id 0 (from 0) to 2 → active id1 shifts down to index 0.
        g.reorder(0, 2);
        assert_eq!(g.active_window().unwrap().id, 1);
        assert_eq!(g.active_index(), 0);
    }

    #[test]
    fn reorder_rejects_invalid_and_same() {
        let mut g = group_with(3);
        assert!(!g.reorder(0, 0));
        assert!(!g.reorder(0, 9));
        assert!(!g.reorder(9, 0));
        assert!(!MuxWindowGroup::new().reorder(0, 1));
    }

    #[test]
    fn reorder_paneids_track_windows() {
        let mut g = group_with(3); // panes 100,101,102
        g.reorder(0, 2); // windows 1,2,0 → panes 101,102,100
        assert_eq!(g.pane_ids(), &[101, 102, 100]);
    }

    // ── pending-create accounting (TS-7) ─────────────────────────────────

    #[test]
    fn pending_create_gates_pane_created() {
        let mut g = MuxWindowGroup::new();
        assert!(!g.take_pending_create()); // no credit → ignore
        g.inc_pending_create();
        assert_eq!(g.pending_create(), 1);
        assert!(g.take_pending_create());
        assert_eq!(g.pending_create(), 0);
        assert!(!g.take_pending_create());
    }

    // ── stable-id re-resolve (TS-14) ─────────────────────────────────────

    #[test]
    fn index_of_window_id_after_reorder() {
        let mut g = group_with(3);
        g.reorder(0, 2); // order ids: 1,2,0
        assert_eq!(g.index_of_window_id(0), Some(2));
        assert_eq!(g.index_of_window_id(2), Some(1));
        assert_eq!(g.index_of_window_id(999), None);
    }

    #[test]
    fn index_of_window_id_none_after_close() {
        let mut g = group_with(3); // panes 100,101,102 ids 0,1,2
        g.remove_pane(101); // close window id 1
        assert_eq!(g.index_of_window_id(1), None);
    }
}
