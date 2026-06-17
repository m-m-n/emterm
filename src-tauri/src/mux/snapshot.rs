//! Snapshot store for mux daemon.
//!
//! Stores WASM grid snapshots + TypeScript-side metadata per pane.
//! Used during detach/reattach to restore terminal state.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use super::session::pane::PaneId;

/// TypeScript-side metadata stored alongside the WASM snapshot.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PaneMetadata {
    pub title: String,
    pub cwd: String,
}

/// A complete pane snapshot: WASM binary + TS metadata.
#[derive(Debug, Clone)]
pub struct PaneSnapshot {
    /// Serialized WASM TerminalCore state (bincode + version envelope).
    pub wasm_state: Vec<u8>,
    /// TypeScript-side metadata (title, CWD).
    pub metadata: PaneMetadata,
}

/// In-memory store for pane snapshots.
pub struct SnapshotStore {
    snapshots: HashMap<PaneId, PaneSnapshot>,
}

impl SnapshotStore {
    pub fn new() -> Self {
        Self {
            snapshots: HashMap::new(),
        }
    }

    /// Save a snapshot for a pane.
    pub fn save(&mut self, pane_id: PaneId, snapshot: PaneSnapshot) {
        self.snapshots.insert(pane_id, snapshot);
    }

    /// Take a snapshot (remove from store).
    pub fn take(&mut self, pane_id: PaneId) -> Option<PaneSnapshot> {
        self.snapshots.remove(&pane_id)
    }

    /// Check if a snapshot exists for a pane.
    pub fn has(&self, pane_id: PaneId) -> bool {
        self.snapshots.contains_key(&pane_id)
    }

    /// Remove a snapshot.
    pub fn remove(&mut self, pane_id: PaneId) {
        self.snapshots.remove(&pane_id);
    }

    /// Clear all snapshots.
    pub fn clear(&mut self) {
        self.snapshots.clear();
    }
}

impl Default for SnapshotStore {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_snapshot() -> PaneSnapshot {
        PaneSnapshot {
            wasm_state: vec![1, 2, 3, 4],
            metadata: PaneMetadata {
                title: "test".to_string(),
                cwd: "/home/user".to_string(),
            },
        }
    }

    #[test]
    fn test_save_and_take() {
        let mut store = SnapshotStore::new();
        store.save(1, test_snapshot());
        assert!(store.has(1));

        let snapshot = store.take(1).unwrap();
        assert_eq!(snapshot.metadata.title, "test");
        assert!(!store.has(1));
    }

    #[test]
    fn test_remove() {
        let mut store = SnapshotStore::new();
        store.save(1, test_snapshot());
        store.remove(1);
        assert!(!store.has(1));
    }

    #[test]
    fn test_clear() {
        let mut store = SnapshotStore::new();
        store.save(1, test_snapshot());
        store.save(2, test_snapshot());
        store.clear();
        assert!(!store.has(1));
        assert!(!store.has(2));
    }
}
