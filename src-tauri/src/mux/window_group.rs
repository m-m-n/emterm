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

use crate::scroll::ScrollPosition;

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
    /// Per-pane saved scroll position. Parallel to [`Self::windows`] /
    /// [`Self::pane_ids`] (invariant F1): index `i` holds the scroll
    /// position window `i`'s pane was showing when it last became inactive.
    /// `Live` (the default) restores to the bottom; `OffsetFromLive(n)`
    /// restores to that offset. Stored as a third parallel array rather than
    /// a field on [`MuxWindow`] so the window entry's `PartialEq`/`Eq`
    /// (used by the tab-bar render tests) keeps depending only on identity +
    /// name, and so the parallel-array invariant is enforced in the same
    /// mutators that move `windows` / `pane_ids`.
    pane_scrolls: Vec<ScrollPosition>,
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

    /// The saved scroll position of the active pane. Returns `Live` (the
    /// default) when the group is empty so callers never branch on emptiness.
    /// Used by the pane-switch save/restore path to reload the incoming
    /// pane's place after committing the new active index.
    pub fn active_pane_scroll(&self) -> ScrollPosition {
        self.pane_scrolls
            .get(self.active)
            .copied()
            .unwrap_or_default()
    }

    /// Store `scroll` as the active pane's saved scroll position. No-op when
    /// the group is empty (no active pane to write). Used by the pane-switch
    /// save path to persist the outgoing pane's place before the active index
    /// moves. Does not expose the parallel `pane_scrolls` array directly so
    /// the invariant stays enforced inside this type.
    pub fn set_active_pane_scroll(&mut self, scroll: ScrollPosition) {
        if let Some(slot) = self.pane_scrolls.get_mut(self.active) {
            *slot = scroll;
        }
    }

    /// Store `scroll` as the saved scroll position of the pane at `index`.
    /// No-op when `index` is out of range. Used by the inbound-switch
    /// reconcile, which moves the active index inside the daemon handler and
    /// then (from `App::pump_all`) parks the *outgoing* pane's position into
    /// its now-inactive slot by index.
    pub fn set_pane_scroll_at(&mut self, index: usize, scroll: ScrollPosition) {
        if let Some(slot) = self.pane_scrolls.get_mut(index) {
            *slot = scroll;
        }
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
        // Every seeded pane starts at the live bottom (`Live`); the saved
        // offset is rebuilt as the user scrolls and switches away (invariant
        // F1: the scroll array tracks the window count).
        self.pane_scrolls = vec![ScrollPosition::default(); windows.len()];
        self.windows = windows;
        self.pane_ids = pane_ids;
        // F1: all three parallel arrays must be the same length after a seed.
        debug_assert_eq!(
            self.pane_scrolls.len(),
            self.windows.len(),
            "F1: seed pane_scrolls length mismatch"
        );
        self.set_active_clamped(active);
    }

    /// Append a window/pane pair and make it active. Port of the
    /// `handleMuxPaneCreated` push (initial name "Terminal" decided by the
    /// caller). Returns the new (now active) index.
    pub fn push(&mut self, window: MuxWindow, pane_id: u32) -> usize {
        self.windows.push(window);
        self.pane_ids.push(pane_id);
        // New pane starts pinned to the bottom (invariant F1).
        self.pane_scrolls.push(ScrollPosition::default());
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
        // Keep the scroll array parallel (invariant F1).
        self.pane_scrolls.remove(idx);
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
        // Move the saved scroll position with its window (invariant F1).
        let scroll = self.pane_scrolls.remove(from);
        self.pane_scrolls.insert(to, scroll);

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

/// Resolve the `next-agent-window` mux action's cycle target (SPEC
/// mux-agent-tab-cycle FR2/FR3/FR5, IMPLEMENTATION.md cross-task decision
/// 2): a pure decision over a display-ordered, per-window qualifying-flag
/// list and the current window index.
///
/// Scans forward starting at the position after `current`, wrapping once
/// through the full order with `current` itself considered last, and
/// returns the index of the first qualifying (`true`) entry found.
/// Consequences: if `current` is the only qualifying window, the scan
/// returns `current` (active window unchanged, AC-4); if no window
/// qualifies, returns `None` (no-op, AC-5, FR5). Returns `None` for an
/// empty list. Deliberately independent of [`MuxWindowGroup`] (takes plain
/// slices) so it is unit-testable without a GUI context and callable at
/// key-event time with no polling or cached qualify lists (NFR2).
pub fn next_qualifying_index(qualifies: &[bool], current: usize) -> Option<usize> {
    let len = qualifies.len();
    if len == 0 {
        return None;
    }
    (1..=len)
        .map(|offset| (current + offset) % len)
        .find(|&idx| qualifies[idx])
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

    // ── per-pane scroll slot (FR3 pane wiring) ───────────────────────────

    #[test]
    fn pane_scroll_defaults_to_live() {
        let g = group_with(3);
        assert_eq!(g.active_pane_scroll(), ScrollPosition::Live);
        // Empty group resolves to Live as well (no active pane).
        assert_eq!(
            MuxWindowGroup::new().active_pane_scroll(),
            ScrollPosition::Live
        );
    }

    #[test]
    fn pane_scroll_set_get_round_trip_is_per_pane() {
        let mut g = group_with(3); // active 0
        g.set_active_pane_scroll(ScrollPosition::OffsetFromLive(7));
        assert_eq!(g.active_pane_scroll(), ScrollPosition::OffsetFromLive(7));
        // A different pane keeps its own (default) value.
        g.set_active_clamped(1);
        assert_eq!(g.active_pane_scroll(), ScrollPosition::Live);
        // Returning to pane 0 restores its saved offset.
        g.set_active_clamped(0);
        assert_eq!(g.active_pane_scroll(), ScrollPosition::OffsetFromLive(7));
    }

    #[test]
    fn pane_scroll_set_on_empty_group_is_noop() {
        let mut g = MuxWindowGroup::new();
        g.set_active_pane_scroll(ScrollPosition::OffsetFromLive(3));
        assert_eq!(g.active_pane_scroll(), ScrollPosition::Live);
    }

    #[test]
    fn pane_scroll_follows_reorder_and_survives_remove() {
        let mut g = group_with(3); // panes 100,101,102; active 0
        g.set_active_clamped(1);
        g.set_active_pane_scroll(ScrollPosition::OffsetFromLive(9)); // pane 101
        // Reorder window at 1 to index 0 → pane 101 leads, scroll travels.
        g.reorder(1, 0);
        assert_eq!(g.pane_ids()[0], 101);
        g.set_active_clamped(0);
        assert_eq!(g.active_pane_scroll(), ScrollPosition::OffsetFromLive(9));
        // The scroll array stays parallel after a remove.
        g.remove_pane(100);
        assert_eq!(g.windows().len(), g.pane_ids().len());
    }

    #[test]
    fn push_resets_new_pane_scroll_to_live() {
        let mut g = group_with(2);
        g.set_active_clamped(0);
        g.set_active_pane_scroll(ScrollPosition::OffsetFromLive(4));
        g.push(win(9, "extra"), 109); // becomes active
        assert_eq!(g.active_pane_scroll(), ScrollPosition::Live);
        // The previously-scrolled pane keeps its offset.
        g.set_active_clamped(0);
        assert_eq!(g.active_pane_scroll(), ScrollPosition::OffsetFromLive(4));
    }

    // ── next_qualifying_index (mux-agent-tab-cycle task0001 TS-1 … TS-5) ──

    /// AC-2 (TS-1, TS-2): with a subset of qualifying windows, repeated
    /// invocations visit exactly the qualifying windows in display order,
    /// skipping non-qualifying ones.
    #[test]
    fn next_qualifying_index_skips_non_qualifying_in_display_order() {
        // windows 0..5; only 1 and 3 qualify.
        let qualifies = [false, true, false, true, false];
        assert_eq!(next_qualifying_index(&qualifies, 0), Some(1));
        assert_eq!(next_qualifying_index(&qualifies, 1), Some(3));
        assert_eq!(next_qualifying_index(&qualifies, 2), Some(3));
    }

    /// AC-2/AC-3: a full repeated-invocation walk cycles through exactly
    /// the qualifying windows, wrapping back to the first once the last
    /// qualifying window is reached.
    #[test]
    fn next_qualifying_index_repeated_invocation_cycles_qualifying_only() {
        let qualifies = [false, true, false, true, false];
        let mut current = 0usize;
        let mut visited = Vec::new();
        for _ in 0..4 {
            let next = next_qualifying_index(&qualifies, current).unwrap();
            visited.push(next);
            current = next;
        }
        assert_eq!(visited, vec![1, 3, 1, 3]);
    }

    /// AC-3 (TS-3): invoking from the last qualifying window in display
    /// order wraps around and lands on the first qualifying window.
    #[test]
    fn next_qualifying_index_wraps_from_last_qualifying_to_first() {
        let qualifies = [false, true, false, true, false];
        // Window 3 is the last qualifying window in display order.
        assert_eq!(next_qualifying_index(&qualifies, 3), Some(1));
    }

    /// AC-4 (TS-4): with exactly one qualifying window, invocation lands
    /// on (or stays on) that window regardless of the starting index.
    #[test]
    fn next_qualifying_index_single_qualifying_window_is_stable() {
        let qualifies = [false, true, false];
        assert_eq!(next_qualifying_index(&qualifies, 1), Some(1));
        assert_eq!(next_qualifying_index(&qualifies, 0), Some(1));
        assert_eq!(next_qualifying_index(&qualifies, 2), Some(1));
    }

    /// AC-5 (TS-5): with zero qualifying windows, there is no target.
    #[test]
    fn next_qualifying_index_zero_qualifying_returns_none() {
        let qualifies = [false, false, false];
        assert_eq!(next_qualifying_index(&qualifies, 0), None);
    }

    #[test]
    fn next_qualifying_index_empty_list_returns_none() {
        assert_eq!(next_qualifying_index(&[], 0), None);
    }
}
