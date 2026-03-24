//! Session state: contains windows, tracks active window.

use std::collections::BTreeMap;

use super::window::{MuxWindow, WindowId};

/// Session identifier.
pub type SessionId = u32;

/// A terminal multiplexer session containing one or more windows.
pub struct MuxSession {
    pub id: SessionId,
    pub name: String,
    pub windows: BTreeMap<WindowId, MuxWindow>,
    pub active_window_id: Option<WindowId>,
    next_window_id: WindowId,
}

impl MuxSession {
    pub fn new(id: SessionId, name: String) -> Self {
        Self {
            id,
            name,
            windows: BTreeMap::new(),
            active_window_id: None,
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
        if self.active_window_id == Some(window_id) {
            self.active_window_id = self.windows.keys().next().copied();
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
}
