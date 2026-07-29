//! Window state: contains panes, tracks active pane.

use std::collections::BTreeMap;

use super::pane::{MuxPane, PaneId};

/// Window identifier.
pub type WindowId = u32;

/// A terminal window containing one or more panes.
pub struct MuxWindow {
    pub id: WindowId,
    pub name: String,
    pub panes: BTreeMap<PaneId, MuxPane>,
    pub active_pane_id: Option<PaneId>,
    next_pane_id: PaneId,
}

impl MuxWindow {
    pub fn new(id: WindowId, name: String) -> Self {
        Self {
            id,
            name,
            panes: BTreeMap::new(),
            active_pane_id: None,
            next_pane_id: 1,
        }
    }

    /// Add a pane to this window. Returns the pane_id.
    pub fn add_pane(&mut self, pane: MuxPane) -> PaneId {
        let id = pane.id;
        if self.active_pane_id.is_none() {
            self.active_pane_id = Some(id);
        }
        self.panes.insert(id, pane);
        id
    }

    /// Allocate the next pane ID for this window.
    pub fn alloc_pane_id(&mut self) -> PaneId {
        let id = self.next_pane_id;
        self.next_pane_id += 1;
        id
    }

    /// Remove a pane by ID.
    pub fn remove_pane(&mut self, pane_id: PaneId) -> Option<MuxPane> {
        let pane = self.panes.remove(&pane_id);
        if self.active_pane_id == Some(pane_id) {
            self.active_pane_id = self.panes.keys().next().copied();
        }
        pane
    }

    /// Check if this window has no panes.
    pub fn is_empty(&self) -> bool {
        self.panes.is_empty()
    }

    /// Get pane count.
    pub fn pane_count(&self) -> usize {
        self.panes.len()
    }

    /// The pane id [`Self::alloc_pane_id`] will allocate next (task0003
    /// AC-1/AC-3): a read-only snapshot accessor for the handoff document's
    /// per-window pane-id counter.
    pub fn next_pane_id_counter(&self) -> PaneId {
        self.next_pane_id
    }

    /// Construct a window directly from restored parts (task0003 AC-1/
    /// AC-3): the pane tree, active selection and the pane-id counter are
    /// all set VERBATIM from the handoff document, rather than rebuilt
    /// incrementally through [`Self::add_pane`] / [`Self::alloc_pane_id`].
    pub fn from_restored(
        id: WindowId,
        name: String,
        panes: BTreeMap<PaneId, MuxPane>,
        active_pane_id: Option<PaneId>,
        next_pane_id: PaneId,
    ) -> Self {
        Self {
            id,
            name,
            panes,
            active_pane_id,
            next_pane_id,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mux::session::pane::{MuxPane, PaneOutputTarget, SharedOutputTarget};
    use std::sync::{Arc, Mutex as StdMutex};
    use tokio::sync::mpsc;

    fn make_pane(id: PaneId) -> MuxPane {
        let (tx, _rx) = mpsc::channel(1);
        let target: SharedOutputTarget = Arc::new(StdMutex::new(PaneOutputTarget::Connected(tx)));
        MuxPane::new_test(id, 80, 24, target)
    }

    // ── task0003 AC-1/AC-3: restore-oriented reconstruction ──────────────

    /// AC-1: `from_restored` sets every field verbatim, including the
    /// pane-id counter, rather than rebuilding via `add_pane`.
    #[test]
    fn from_restored_sets_all_fields_verbatim() {
        let mut panes = BTreeMap::new();
        panes.insert(10, make_pane(10));
        panes.insert(20, make_pane(20));
        let w = MuxWindow::from_restored(1, "restored".to_string(), panes, Some(20), 99);
        assert_eq!(w.id, 1);
        assert_eq!(w.name, "restored");
        assert_eq!(w.active_pane_id, Some(20));
        assert_eq!(w.next_pane_id_counter(), 99);
        assert!(w.panes.contains_key(&10));
        assert!(w.panes.contains_key(&20));
    }

    /// AC-3: the next pane id allocated after restore continues the
    /// original sequence rather than restarting from 1.
    #[test]
    fn from_restored_next_pane_id_continues_the_original_sequence() {
        let mut w = MuxWindow::from_restored(1, "w".to_string(), BTreeMap::new(), None, 50);
        assert_eq!(w.alloc_pane_id(), 50);
        assert_eq!(w.alloc_pane_id(), 51);
    }

    /// AC-1: the snapshot accessor reports exactly what a plain `new()` +
    /// allocations would have advanced the counter to.
    #[test]
    fn next_pane_id_counter_matches_plain_constructor_after_allocations() {
        let mut w = MuxWindow::new(1, "w".to_string());
        w.alloc_pane_id();
        w.alloc_pane_id();
        assert_eq!(w.next_pane_id_counter(), 3);
    }
}
