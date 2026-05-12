//! Top-level `App` state container.
//!
//! Phase 1 was empty. Phase 2 holds a PTY-backed tab and offers an
//! `on_resize` hook that propagates the new grid dimensions to all PTYs.
//! Later phases extend this with a viewer registry and settings.

use crate::selection::Selection;
use crate::tabs::Tab;

pub struct App {
    pub tabs: Vec<Tab>,
    pub active: usize,
    /// Last known grid size in cells. Updated by `window_host` whenever the
    /// window resizes; PTYs are resized to match.
    pub cell_size: GridDims,
    /// Active mouse selection on the active tab, if any. Phase 4 owns this;
    /// later phases may move it per-tab when tabs preserve selection.
    pub selection: Option<Selection>,
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
        Self {
            tabs: Vec::new(),
            active: 0,
            cell_size: GridDims::default(),
            selection: None,
        }
    }

    /// Spawn the initial shell tab. Called once at startup.
    pub fn spawn_initial_tab(&mut self) {
        let dims = self.cell_size;
        let tab = Tab::spawn_shell("shell", dims.cols, dims.rows);
        self.tabs.push(tab);
        self.active = 0;
    }

    pub fn has_tabs(&self) -> bool {
        !self.tabs.is_empty()
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
            if tab.pump() {
                changed = true;
            }
        }
        // Reap exited tabs (Phase 5 will refine the policy).
        let before = self.tabs.len();
        self.tabs.retain(|t| !t.exited);
        if self.tabs.len() != before {
            if self.active >= self.tabs.len() && !self.tabs.is_empty() {
                self.active = self.tabs.len() - 1;
            }
            changed = true;
        }
        changed
    }

    /// Propagate a new grid size to all PTYs.
    pub fn set_grid_size(&mut self, cols: u16, rows: u16) {
        self.cell_size = GridDims { cols, rows };
        for tab in &self.tabs {
            tab.resize(cols, rows);
        }
    }
}

impl Default for App {
    fn default() -> Self {
        Self::new()
    }
}
