//! Session state: contains windows, tracks active window.

use std::collections::BTreeMap;

use tokio::sync::oneshot;

use super::pane::MuxPane;
use super::window::{MuxWindow, WindowId};

/// Session identifier.
pub type SessionId = u32;

/// A terminal multiplexer session containing one or more windows.
pub struct MuxSession {
    pub id: SessionId,
    pub name: String,
    /// Window lookup by ID (used for random access).
    pub windows: BTreeMap<WindowId, MuxWindow>,
    /// Explicit window ordering for display and reorder operations.
    ///
    /// Invariant: `window_order` and `windows` keys always reference the
    /// same set of `WindowId`s. Enforced by `add_window` / `remove_window` /
    /// `move_window`.
    pub window_order: Vec<WindowId>,
    pub active_window_id: Option<WindowId>,
    /// Signal handle for the currently attached GUI client.
    ///
    /// When another client attaches to the same session, the new `collect_reattach_data`
    /// takes this sender and fires it, which causes the previous client's connection
    /// loop to send `Detached` and exit. The connection handler treats an `Err` on the
    /// receiver (sender dropped without send) as a no-op, so a client that cleanly
    /// switches to a different session is not kicked out of the one it is leaving.
    pub active_client_kick: Option<oneshot::Sender<()>>,
    next_window_id: WindowId,
}

impl MuxSession {
    pub fn new(id: SessionId, name: String) -> Self {
        Self {
            id,
            name,
            windows: BTreeMap::new(),
            window_order: Vec::new(),
            active_window_id: None,
            active_client_kick: None,
            next_window_id: 1,
        }
    }

    /// Add a window to this session.
    pub fn add_window(&mut self, window: MuxWindow) -> WindowId {
        let id = window.id;
        if self.active_window_id.is_none() {
            self.active_window_id = Some(id);
        }
        self.windows.insert(id, window);
        // Keep window_order consistent: append new IDs to the end. If the id
        // already exists (shouldn't happen with alloc_window_id), keep the
        // existing position instead of duplicating.
        if !self.window_order.contains(&id) {
            self.window_order.push(id);
        }
        id
    }

    /// Allocate the next window ID.
    pub fn alloc_window_id(&mut self) -> WindowId {
        let id = self.next_window_id;
        self.next_window_id += 1;
        id
    }

    /// Remove a window by ID.
    pub fn remove_window(&mut self, window_id: WindowId) -> Option<MuxWindow> {
        let window = self.windows.remove(&window_id);
        self.window_order.retain(|&id| id != window_id);
        if self.active_window_id == Some(window_id) {
            // Select the first window in display order as the new active
            // window. This is a behavior change from the previous
            // BTreeMap-key-based selection: when windows are created in the
            // order [id=2, id=1], removing id=2 now activates id=1 (the
            // first in display order) rather than the lowest ID.
            self.active_window_id = self.window_order.first().copied();
        }
        window
    }

    /// Check if this session has no windows.
    pub fn is_empty(&self) -> bool {
        self.windows.is_empty()
    }

    /// Get total pane count across all windows.
    pub fn pane_count(&self) -> usize {
        self.windows.values().map(|w| w.pane_count()).sum()
    }

    /// Get window count.
    pub fn window_count(&self) -> usize {
        self.windows.len()
    }

    /// Iterate over every pane in this session, across all windows, paired
    /// with its window id.
    ///
    /// Used by the post-snapshot agent-status sync (SPEC FR4/FR5): after a
    /// client receives a snapshot (attach / window switch), the daemon walks
    /// every pane in the session to find the ones with a reported state and
    /// resends their status out-of-band (`replay_derived: true`).
    pub fn panes_iter(&self) -> impl Iterator<Item = (WindowId, &MuxPane)> {
        self.windows
            .iter()
            .flat_map(|(&wid, w)| w.panes.values().map(move |p| (wid, p)))
    }

    /// Reorder an existing window to `target_index` within `window_order`.
    ///
    /// Semantics (remove-then-insert):
    /// - The window is removed from its current position, then inserted
    ///   at the clamped target. `target_index` is clamped to
    ///   `[0, window_order.len() - 1]` (where `len` is measured *after*
    ///   the removal, i.e., equal to the original `len - 1`).
    /// - If the clamped target equals the current position, the order
    ///   is unchanged and `false` is returned.
    /// - `active_window_id` is preserved (never modified).
    ///
    /// Returns `true` iff the order was actually changed.
    pub fn move_window(&mut self, window_id: WindowId, target_index: usize) -> bool {
        let Some(cur) = self.window_order.iter().position(|&id| id == window_id) else {
            return false;
        };
        let len_after_remove = self.window_order.len().saturating_sub(1);
        if len_after_remove == 0 {
            // Only one window -- no reordering possible.
            return false;
        }
        let clamped = target_index.min(len_after_remove);
        if clamped == cur {
            return false;
        }
        let id = self.window_order.remove(cur);
        self.window_order.insert(clamped, id);
        true
    }

    /// The window id [`Self::alloc_window_id`] will allocate next (task0003
    /// AC-1/AC-3): a read-only snapshot accessor for the handoff document's
    /// per-session window-id counter.
    pub fn next_window_id_counter(&self) -> WindowId {
        self.next_window_id
    }

    /// Construct a session directly from restored parts (task0003 AC-1/
    /// AC-3): the window tree, ordering, active selection and the
    /// window-id counter are all set VERBATIM from the handoff document,
    /// rather than rebuilt incrementally through [`Self::add_window`] /
    /// [`Self::alloc_window_id`].
    #[allow(clippy::too_many_arguments)]
    pub fn from_restored(
        id: SessionId,
        name: String,
        windows: BTreeMap<WindowId, MuxWindow>,
        window_order: Vec<WindowId>,
        active_window_id: Option<WindowId>,
        next_window_id: WindowId,
    ) -> Self {
        Self {
            id,
            name,
            windows,
            window_order,
            active_window_id,
            active_client_kick: None,
            next_window_id,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mux::session::window::MuxWindow;

    fn make_window(id: WindowId, name: &str) -> MuxWindow {
        MuxWindow::new(id, name.to_string())
    }

    #[test]
    fn test_panes_iter_covers_all_windows() {
        use crate::mux::session::pane::{MuxPane, PaneOutputTarget, SharedOutputTarget};
        use std::sync::{Arc as StdArc, Mutex as StdMutex};
        use tokio::sync::mpsc;

        fn test_pane(id: u32) -> MuxPane {
            let (tx, _rx) = mpsc::channel(1);
            let target: SharedOutputTarget =
                StdArc::new(StdMutex::new(PaneOutputTarget::Connected(tx)));
            MuxPane::new_test(id, 80, 24, target)
        }

        let mut s = MuxSession::new(1, "s".to_string());
        let w1 = s.add_window(make_window(10, "a"));
        let w2 = s.add_window(make_window(20, "b"));
        s.windows.get_mut(&w1).unwrap().add_pane(test_pane(100));
        s.windows.get_mut(&w2).unwrap().add_pane(test_pane(200));
        s.windows.get_mut(&w2).unwrap().add_pane(test_pane(201));

        let mut found: Vec<(WindowId, u32)> = s.panes_iter().map(|(wid, p)| (wid, p.id)).collect();
        found.sort();
        assert_eq!(found, vec![(10, 100), (20, 200), (20, 201)]);
    }

    #[test]
    fn test_panes_iter_empty_session() {
        let s = MuxSession::new(1, "s".to_string());
        assert_eq!(s.panes_iter().count(), 0);
    }

    #[test]
    fn test_window_order_after_adds() {
        let mut s = MuxSession::new(1, "s".to_string());
        s.add_window(make_window(10, "a"));
        s.add_window(make_window(20, "b"));
        s.add_window(make_window(30, "c"));
        assert_eq!(s.window_order, vec![10, 20, 30]);
        // windows BTreeMap retains the same set
        assert!(s.windows.contains_key(&10));
        assert!(s.windows.contains_key(&20));
        assert!(s.windows.contains_key(&30));
    }

    #[test]
    fn test_window_order_after_removes() {
        let mut s = MuxSession::new(1, "s".to_string());
        s.add_window(make_window(10, "a"));
        s.add_window(make_window(20, "b"));
        s.add_window(make_window(30, "c"));
        s.add_window(make_window(40, "d"));

        // Remove middle
        s.remove_window(20);
        assert_eq!(s.window_order, vec![10, 30, 40]);
        // Remove head
        s.remove_window(10);
        assert_eq!(s.window_order, vec![30, 40]);
        // Remove tail
        s.remove_window(40);
        assert_eq!(s.window_order, vec![30]);
        // Remove last
        s.remove_window(30);
        assert!(s.window_order.is_empty());
        assert!(s.windows.is_empty());
    }

    #[test]
    fn test_active_window_id_after_remove_uses_order() {
        // Add windows in the order [A(id=2), B(id=1)] to prove that
        // the new active selection uses window_order (first in display
        // order), not BTreeMap ordering (lowest ID).
        let mut s = MuxSession::new(1, "s".to_string());
        s.add_window(make_window(2, "A"));
        s.add_window(make_window(1, "B"));
        assert_eq!(s.window_order, vec![2, 1]);
        // A (id=2) is initially active (first window added).
        assert_eq!(s.active_window_id, Some(2));

        // Remove A: window_order.first() is now B (id=1).
        s.remove_window(2);
        assert_eq!(s.active_window_id, Some(1));
    }

    #[test]
    fn test_active_window_id_none_after_all_removed() {
        let mut s = MuxSession::new(1, "s".to_string());
        s.add_window(make_window(1, "a"));
        assert_eq!(s.active_window_id, Some(1));
        s.remove_window(1);
        assert_eq!(s.active_window_id, None);
    }

    // ---- move_window tests (Phase 3) ----

    fn make_session_with_ids(ids: &[WindowId]) -> MuxSession {
        let mut s = MuxSession::new(1, "s".to_string());
        for &id in ids {
            s.add_window(make_window(id, &format!("w{}", id)));
        }
        s
    }

    #[test]
    fn test_move_window_to_first() {
        // [A,B,C,D] -> move D (id=4) to 0 -> [D,A,B,C]
        let mut s = make_session_with_ids(&[1, 2, 3, 4]);
        assert!(s.move_window(4, 0));
        assert_eq!(s.window_order, vec![4, 1, 2, 3]);
    }

    #[test]
    fn test_move_window_to_last() {
        // [A,B,C,D] -> move A (id=1) to 3 -> [B,C,D,A]
        let mut s = make_session_with_ids(&[1, 2, 3, 4]);
        assert!(s.move_window(1, 3));
        assert_eq!(s.window_order, vec![2, 3, 4, 1]);
    }

    #[test]
    fn test_move_window_to_middle_forward() {
        // [A,B,C,D] -> move B (id=2) to 2 -> [A,C,B,D]
        // (remove-then-insert: after removing B, list is [A,C,D]; insert at 2 -> [A,C,B,D])
        let mut s = make_session_with_ids(&[1, 2, 3, 4]);
        assert!(s.move_window(2, 2));
        assert_eq!(s.window_order, vec![1, 3, 2, 4]);
    }

    #[test]
    fn test_move_window_to_middle_backward() {
        // [A,B,C,D] -> move D (id=4) to 1 -> [A,D,B,C]
        let mut s = make_session_with_ids(&[1, 2, 3, 4]);
        assert!(s.move_window(4, 1));
        assert_eq!(s.window_order, vec![1, 4, 2, 3]);
    }

    #[test]
    fn test_move_window_same_position() {
        let mut s = make_session_with_ids(&[1, 2, 3]);
        let before = s.window_order.clone();
        // B (id=2) is at index 1; move to 1 -> no-op
        assert!(!s.move_window(2, 1));
        assert_eq!(s.window_order, before);
    }

    #[test]
    fn test_move_window_out_of_range_clamps() {
        // [1,2,3] -> move 1 to 999 -> clamp to 2 -> [2,3,1]
        let mut s = make_session_with_ids(&[1, 2, 3]);
        assert!(s.move_window(1, 999));
        assert_eq!(s.window_order, vec![2, 3, 1]);
    }

    #[test]
    fn test_move_window_unknown_id() {
        let mut s = make_session_with_ids(&[1, 2, 3]);
        let before = s.window_order.clone();
        assert!(!s.move_window(99, 0));
        assert_eq!(s.window_order, before);
    }

    #[test]
    fn test_move_window_preserves_active() {
        let mut s = make_session_with_ids(&[1, 2, 3]);
        // Active is first-added = 1
        assert_eq!(s.active_window_id, Some(1));
        // Move active (id=1) to last
        assert!(s.move_window(1, 2));
        // active_window_id should NOT change, even though its index moved
        assert_eq!(s.active_window_id, Some(1));
        assert_eq!(s.window_order, vec![2, 3, 1]);
    }

    #[test]
    fn test_move_window_single_window_noop() {
        let mut s = make_session_with_ids(&[1]);
        assert!(!s.move_window(1, 0));
        assert_eq!(s.window_order, vec![1]);
    }

    #[test]
    fn test_move_window_windows_btreemap_unchanged() {
        let mut s = make_session_with_ids(&[1, 2, 3]);
        let keys_before: Vec<WindowId> = s.windows.keys().copied().collect();
        assert!(s.move_window(3, 0));
        let keys_after: Vec<WindowId> = s.windows.keys().copied().collect();
        assert_eq!(keys_before, keys_after);
        // window_order is reordered
        assert_eq!(s.window_order, vec![3, 1, 2]);
    }

    // ── task0003 AC-1/AC-3: restore-oriented reconstruction ──────────────

    /// AC-1: `from_restored` sets every field verbatim, including the
    /// window-id counter, rather than rebuilding via `add_window`.
    #[test]
    fn from_restored_sets_all_fields_verbatim() {
        let mut windows = BTreeMap::new();
        windows.insert(10, make_window(10, "a"));
        windows.insert(20, make_window(20, "b"));
        let s = MuxSession::from_restored(1, "restored".to_string(), windows, vec![20, 10], Some(20), 99);
        assert_eq!(s.id, 1);
        assert_eq!(s.name, "restored");
        assert_eq!(s.window_order, vec![20, 10]);
        assert_eq!(s.active_window_id, Some(20));
        assert_eq!(s.next_window_id_counter(), 99);
        assert!(s.windows.contains_key(&10));
        assert!(s.windows.contains_key(&20));
    }

    /// AC-3: the next window id allocated after restore continues the
    /// original sequence rather than restarting from 1.
    #[test]
    fn from_restored_next_window_id_continues_the_original_sequence() {
        let mut s = MuxSession::from_restored(1, "s".to_string(), BTreeMap::new(), vec![], None, 50);
        assert_eq!(s.alloc_window_id(), 50);
        assert_eq!(s.alloc_window_id(), 51);
    }

    /// AC-1: the snapshot accessor reports exactly what a plain `new()` +
    /// allocations would have advanced the counter to.
    #[test]
    fn next_window_id_counter_matches_plain_constructor_after_allocations() {
        let mut s = MuxSession::new(1, "s".to_string());
        s.alloc_window_id();
        s.alloc_window_id();
        assert_eq!(s.next_window_id_counter(), 3);
    }
}
