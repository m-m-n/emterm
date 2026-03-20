//! Window state: contains panes, tracks active pane.

use std::collections::HashMap;

use super::pane::{MuxPane, PaneId};

/// Window identifier.
pub type WindowId = u32;

/// A terminal window containing one or more panes.
pub struct MuxWindow {
    pub id: WindowId,
    pub name: String,
    pub panes: HashMap<PaneId, MuxPane>,
    pub active_pane_id: Option<PaneId>,
    next_pane_id: PaneId,
}

impl MuxWindow {
    pub fn new(id: WindowId, name: String) -> Self {
        Self {
            id,
            name,
            panes: HashMap::new(),
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
}
