//! Placement management for image display positions.
//!
//! Tracks image placements on screen with support for various query criteria.
//!
//! # Example
//!
//! ```
//! use term_images::image_proc::placement::PlacementManager;
//! use term_images::image_proc::ImagePlacement;
//!
//! let mut manager = PlacementManager::new();
//!
//! let placement = ImagePlacement {
//!     image_id: 1,
//!     placement_id: 1,
//!     row: 5,
//!     col: 10,
//!     ..Default::default()
//! };
//!
//! manager.add(placement);
//! assert!(manager.contains(1, 1));
//! ```

use std::collections::{BTreeMap, HashMap, HashSet};

use super::ImagePlacement;

/// Key for placement lookup: (image_id, placement_id).
type PlacementKey = (u32, u32);

/// Cell position for spatial queries.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct CellPosition {
    /// Row (0-based).
    pub row: u32,
    /// Column (0-based).
    pub col: u32,
}

impl CellPosition {
    /// Create a new cell position.
    pub fn new(row: u32, col: u32) -> Self {
        Self { row, col }
    }
}

/// Placement manager for tracking image positions.
pub struct PlacementManager {
    /// All placements by key (image_id, placement_id).
    placements: HashMap<PlacementKey, ImagePlacement>,

    /// Placements indexed by image ID.
    by_image_id: HashMap<u32, HashSet<u32>>,

    /// Placements indexed by z-index.
    by_z_index: BTreeMap<i32, HashSet<PlacementKey>>,

    /// Placements indexed by cell position (row, col).
    by_position: HashMap<CellPosition, HashSet<PlacementKey>>,

    /// Placements indexed by row.
    by_row: HashMap<u32, HashSet<PlacementKey>>,

    /// Placements indexed by column.
    by_column: HashMap<u32, HashSet<PlacementKey>>,
}

impl Default for PlacementManager {
    fn default() -> Self {
        Self::new()
    }
}

impl PlacementManager {
    /// Create a new placement manager.
    pub fn new() -> Self {
        Self {
            placements: HashMap::new(),
            by_image_id: HashMap::new(),
            by_z_index: BTreeMap::new(),
            by_position: HashMap::new(),
            by_row: HashMap::new(),
            by_column: HashMap::new(),
        }
    }

    /// Add a placement.
    ///
    /// If a placement with the same key already exists, it will be replaced.
    pub fn add(&mut self, placement: ImagePlacement) {
        let key = (placement.image_id, placement.placement_id);

        // Remove existing placement if present
        if self.placements.contains_key(&key) {
            self.remove(placement.image_id, placement.placement_id);
        }

        let pos = CellPosition::new(placement.row, placement.col);

        // Add to indexes
        self.by_image_id
            .entry(placement.image_id)
            .or_default()
            .insert(placement.placement_id);

        self.by_z_index
            .entry(placement.z_index)
            .or_default()
            .insert(key);

        self.by_position.entry(pos).or_default().insert(key);

        self.by_row.entry(placement.row).or_default().insert(key);

        self.by_column.entry(placement.col).or_default().insert(key);

        // Store the placement
        self.placements.insert(key, placement);
    }

    /// Get a placement by key.
    pub fn get(&self, image_id: u32, placement_id: u32) -> Option<&ImagePlacement> {
        self.placements.get(&(image_id, placement_id))
    }

    /// Check if a placement exists.
    pub fn contains(&self, image_id: u32, placement_id: u32) -> bool {
        self.placements.contains_key(&(image_id, placement_id))
    }

    /// Remove a specific placement.
    pub fn remove(&mut self, image_id: u32, placement_id: u32) -> Option<ImagePlacement> {
        let key = (image_id, placement_id);

        if let Some(placement) = self.placements.remove(&key) {
            let pos = CellPosition::new(placement.row, placement.col);

            // Remove from indexes
            if let Some(ids) = self.by_image_id.get_mut(&image_id) {
                ids.remove(&placement_id);
                if ids.is_empty() {
                    self.by_image_id.remove(&image_id);
                }
            }

            if let Some(keys) = self.by_z_index.get_mut(&placement.z_index) {
                keys.remove(&key);
                if keys.is_empty() {
                    self.by_z_index.remove(&placement.z_index);
                }
            }

            if let Some(keys) = self.by_position.get_mut(&pos) {
                keys.remove(&key);
                if keys.is_empty() {
                    self.by_position.remove(&pos);
                }
            }

            if let Some(keys) = self.by_row.get_mut(&placement.row) {
                keys.remove(&key);
                if keys.is_empty() {
                    self.by_row.remove(&placement.row);
                }
            }

            if let Some(keys) = self.by_column.get_mut(&placement.col) {
                keys.remove(&key);
                if keys.is_empty() {
                    self.by_column.remove(&placement.col);
                }
            }

            Some(placement)
        } else {
            None
        }
    }

    /// Remove all placements for an image.
    pub fn remove_by_image_id(&mut self, image_id: u32) -> Vec<ImagePlacement> {
        let placement_ids: Vec<u32> = self
            .by_image_id
            .get(&image_id)
            .map(|ids| ids.iter().copied().collect())
            .unwrap_or_default();

        placement_ids
            .into_iter()
            .filter_map(|pid| self.remove(image_id, pid))
            .collect()
    }

    /// Remove all placements at a specific position.
    pub fn remove_at_position(&mut self, row: u32, col: u32) -> Vec<ImagePlacement> {
        let pos = CellPosition::new(row, col);

        let keys: Vec<PlacementKey> = self
            .by_position
            .get(&pos)
            .map(|keys| keys.iter().copied().collect())
            .unwrap_or_default();

        keys.into_iter()
            .filter_map(|(iid, pid)| self.remove(iid, pid))
            .collect()
    }

    /// Remove all placements with a specific z-index.
    pub fn remove_by_z_index(&mut self, z_index: i32) -> Vec<ImagePlacement> {
        let keys: Vec<PlacementKey> = self
            .by_z_index
            .get(&z_index)
            .map(|keys| keys.iter().copied().collect())
            .unwrap_or_default();

        keys.into_iter()
            .filter_map(|(iid, pid)| self.remove(iid, pid))
            .collect()
    }

    /// Remove all placements in a specific row.
    pub fn remove_by_row(&mut self, row: u32) -> Vec<ImagePlacement> {
        let keys: Vec<PlacementKey> = self
            .by_row
            .get(&row)
            .map(|keys| keys.iter().copied().collect())
            .unwrap_or_default();

        keys.into_iter()
            .filter_map(|(iid, pid)| self.remove(iid, pid))
            .collect()
    }

    /// Remove all placements in a specific column.
    pub fn remove_by_column(&mut self, col: u32) -> Vec<ImagePlacement> {
        let keys: Vec<PlacementKey> = self
            .by_column
            .get(&col)
            .map(|keys| keys.iter().copied().collect())
            .unwrap_or_default();

        keys.into_iter()
            .filter_map(|(iid, pid)| self.remove(iid, pid))
            .collect()
    }

    /// Remove all placements.
    pub fn clear(&mut self) -> Vec<ImagePlacement> {
        let placements: Vec<ImagePlacement> = self.placements.values().cloned().collect();

        self.placements.clear();
        self.by_image_id.clear();
        self.by_z_index.clear();
        self.by_position.clear();
        self.by_row.clear();
        self.by_column.clear();

        placements
    }

    /// Get all placements for an image.
    pub fn get_by_image_id(&self, image_id: u32) -> Vec<&ImagePlacement> {
        self.by_image_id
            .get(&image_id)
            .map(|ids| {
                ids.iter()
                    .filter_map(|&pid| self.placements.get(&(image_id, pid)))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Get all placements at a specific position.
    pub fn get_at_position(&self, row: u32, col: u32) -> Vec<&ImagePlacement> {
        let pos = CellPosition::new(row, col);

        self.by_position
            .get(&pos)
            .map(|keys| {
                keys.iter()
                    .filter_map(|key| self.placements.get(key))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Get all placements with a specific z-index.
    pub fn get_by_z_index(&self, z_index: i32) -> Vec<&ImagePlacement> {
        self.by_z_index
            .get(&z_index)
            .map(|keys| {
                keys.iter()
                    .filter_map(|key| self.placements.get(key))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Get all placements sorted by z-index (ascending).
    pub fn get_all_sorted(&self) -> Vec<&ImagePlacement> {
        let mut result: Vec<&ImagePlacement> = self.placements.values().collect();
        result.sort_by_key(|p| p.z_index);
        result
    }

    /// Get number of placements.
    pub fn len(&self) -> usize {
        self.placements.len()
    }

    /// Check if there are no placements.
    pub fn is_empty(&self) -> bool {
        self.placements.is_empty()
    }

    /// Get all unique image IDs with placements.
    pub fn image_ids(&self) -> Vec<u32> {
        self.by_image_id.keys().copied().collect()
    }

    /// Update scroll offset for all placements.
    ///
    /// This adjusts the row position of placements when the terminal scrolls.
    pub fn scroll(&mut self, delta: i32) {
        // Collect all placements to update
        let updates: Vec<(PlacementKey, ImagePlacement)> = self
            .placements
            .iter()
            .map(|(&key, p)| {
                let mut updated = p.clone();
                updated.row = if delta > 0 {
                    p.row.saturating_add(delta as u32)
                } else {
                    p.row.saturating_sub((-delta) as u32)
                };
                (key, updated)
            })
            .collect();

        // Clear and re-add with updated positions
        self.placements.clear();
        self.by_position.clear();
        self.by_row.clear();

        for (key, placement) in updates {
            let pos = CellPosition::new(placement.row, placement.col);

            self.by_position.entry(pos).or_default().insert(key);
            self.by_row.entry(placement.row).or_default().insert(key);

            self.placements.insert(key, placement);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_placement(image_id: u32, placement_id: u32, row: u32, col: u32) -> ImagePlacement {
        ImagePlacement {
            image_id,
            placement_id,
            row,
            col,
            columns: 10,
            rows: 10,
            x_offset: 0,
            y_offset: 0,
            z_index: 0,
        }
    }

    // =========================================================================
    // Basic Operations
    // =========================================================================

    #[test]
    fn test_manager_creation() {
        let manager = PlacementManager::new();
        assert!(manager.is_empty());
        assert_eq!(manager.len(), 0);
    }

    #[test]
    fn test_add_and_get() {
        let mut manager = PlacementManager::new();
        let placement = make_placement(1, 1, 5, 10);

        manager.add(placement.clone());

        assert_eq!(manager.len(), 1);
        assert!(manager.contains(1, 1));

        let retrieved = manager.get(1, 1).unwrap();
        assert_eq!(retrieved.image_id, 1);
        assert_eq!(retrieved.row, 5);
        assert_eq!(retrieved.col, 10);
    }

    #[test]
    fn test_add_replaces_existing() {
        let mut manager = PlacementManager::new();

        let p1 = make_placement(1, 1, 5, 10);
        let mut p2 = make_placement(1, 1, 20, 30);
        p2.z_index = 5;

        manager.add(p1);
        manager.add(p2);

        assert_eq!(manager.len(), 1);

        let retrieved = manager.get(1, 1).unwrap();
        assert_eq!(retrieved.row, 20);
        assert_eq!(retrieved.col, 30);
        assert_eq!(retrieved.z_index, 5);
    }

    #[test]
    fn test_remove() {
        let mut manager = PlacementManager::new();

        manager.add(make_placement(1, 1, 5, 10));
        manager.add(make_placement(1, 2, 15, 20));

        let removed = manager.remove(1, 1);
        assert!(removed.is_some());
        assert_eq!(removed.unwrap().placement_id, 1);

        assert_eq!(manager.len(), 1);
        assert!(!manager.contains(1, 1));
        assert!(manager.contains(1, 2));
    }

    #[test]
    fn test_remove_nonexistent() {
        let mut manager = PlacementManager::new();
        assert!(manager.remove(999, 999).is_none());
    }

    #[test]
    fn test_clear() {
        let mut manager = PlacementManager::new();

        manager.add(make_placement(1, 1, 5, 10));
        manager.add(make_placement(2, 1, 15, 20));
        manager.add(make_placement(2, 2, 25, 30));

        let cleared = manager.clear();
        assert_eq!(cleared.len(), 3);
        assert!(manager.is_empty());
    }

    // =========================================================================
    // Query by Image ID
    // =========================================================================

    #[test]
    fn test_get_by_image_id() {
        let mut manager = PlacementManager::new();

        manager.add(make_placement(1, 1, 5, 10));
        manager.add(make_placement(1, 2, 15, 20));
        manager.add(make_placement(2, 1, 25, 30));

        let placements = manager.get_by_image_id(1);
        assert_eq!(placements.len(), 2);

        let placements = manager.get_by_image_id(2);
        assert_eq!(placements.len(), 1);

        let placements = manager.get_by_image_id(999);
        assert!(placements.is_empty());
    }

    #[test]
    fn test_remove_by_image_id() {
        let mut manager = PlacementManager::new();

        manager.add(make_placement(1, 1, 5, 10));
        manager.add(make_placement(1, 2, 15, 20));
        manager.add(make_placement(2, 1, 25, 30));

        let removed = manager.remove_by_image_id(1);
        assert_eq!(removed.len(), 2);
        assert_eq!(manager.len(), 1);
        assert!(manager.contains(2, 1));
    }

    // =========================================================================
    // Query by Position
    // =========================================================================

    #[test]
    fn test_get_at_position() {
        let mut manager = PlacementManager::new();

        manager.add(make_placement(1, 1, 5, 10));
        manager.add(make_placement(2, 1, 5, 10)); // Same position
        manager.add(make_placement(3, 1, 20, 30));

        let placements = manager.get_at_position(5, 10);
        assert_eq!(placements.len(), 2);

        let placements = manager.get_at_position(20, 30);
        assert_eq!(placements.len(), 1);

        let placements = manager.get_at_position(0, 0);
        assert!(placements.is_empty());
    }

    #[test]
    fn test_remove_at_position() {
        let mut manager = PlacementManager::new();

        manager.add(make_placement(1, 1, 5, 10));
        manager.add(make_placement(2, 1, 5, 10));
        manager.add(make_placement(3, 1, 20, 30));

        let removed = manager.remove_at_position(5, 10);
        assert_eq!(removed.len(), 2);
        assert_eq!(manager.len(), 1);
    }

    // =========================================================================
    // Query by Z-Index
    // =========================================================================

    #[test]
    fn test_get_by_z_index() {
        let mut manager = PlacementManager::new();

        let mut p1 = make_placement(1, 1, 5, 10);
        p1.z_index = -1;
        let mut p2 = make_placement(2, 1, 15, 20);
        p2.z_index = -1;
        let mut p3 = make_placement(3, 1, 25, 30);
        p3.z_index = 1;

        manager.add(p1);
        manager.add(p2);
        manager.add(p3);

        let placements = manager.get_by_z_index(-1);
        assert_eq!(placements.len(), 2);

        let placements = manager.get_by_z_index(1);
        assert_eq!(placements.len(), 1);
    }

    #[test]
    fn test_remove_by_z_index() {
        let mut manager = PlacementManager::new();

        let mut p1 = make_placement(1, 1, 5, 10);
        p1.z_index = -1;
        let mut p2 = make_placement(2, 1, 15, 20);
        p2.z_index = 0;

        manager.add(p1);
        manager.add(p2);

        let removed = manager.remove_by_z_index(-1);
        assert_eq!(removed.len(), 1);
        assert_eq!(manager.len(), 1);
    }

    // =========================================================================
    // Query by Row/Column
    // =========================================================================

    #[test]
    fn test_remove_by_row() {
        let mut manager = PlacementManager::new();

        manager.add(make_placement(1, 1, 5, 10));
        manager.add(make_placement(2, 1, 5, 20));
        manager.add(make_placement(3, 1, 10, 10));

        let removed = manager.remove_by_row(5);
        assert_eq!(removed.len(), 2);
        assert_eq!(manager.len(), 1);
    }

    #[test]
    fn test_remove_by_column() {
        let mut manager = PlacementManager::new();

        manager.add(make_placement(1, 1, 5, 10));
        manager.add(make_placement(2, 1, 15, 10));
        manager.add(make_placement(3, 1, 5, 20));

        let removed = manager.remove_by_column(10);
        assert_eq!(removed.len(), 2);
        assert_eq!(manager.len(), 1);
    }

    // =========================================================================
    // Sorting and Ordering
    // =========================================================================

    #[test]
    fn test_get_all_sorted() {
        let mut manager = PlacementManager::new();

        let mut p1 = make_placement(1, 1, 5, 10);
        p1.z_index = 10;
        let mut p2 = make_placement(2, 1, 15, 20);
        p2.z_index = -5;
        let mut p3 = make_placement(3, 1, 25, 30);
        p3.z_index = 0;

        manager.add(p1);
        manager.add(p2);
        manager.add(p3);

        let sorted = manager.get_all_sorted();
        assert_eq!(sorted.len(), 3);
        assert_eq!(sorted[0].z_index, -5);
        assert_eq!(sorted[1].z_index, 0);
        assert_eq!(sorted[2].z_index, 10);
    }

    // =========================================================================
    // Scroll
    // =========================================================================

    #[test]
    fn test_scroll_positive() {
        let mut manager = PlacementManager::new();

        manager.add(make_placement(1, 1, 5, 10));
        manager.add(make_placement(2, 1, 10, 20));

        manager.scroll(3);

        let p1 = manager.get(1, 1).unwrap();
        assert_eq!(p1.row, 8);

        let p2 = manager.get(2, 1).unwrap();
        assert_eq!(p2.row, 13);
    }

    #[test]
    fn test_scroll_negative() {
        let mut manager = PlacementManager::new();

        manager.add(make_placement(1, 1, 10, 10));

        manager.scroll(-3);

        let p = manager.get(1, 1).unwrap();
        assert_eq!(p.row, 7);
    }

    #[test]
    fn test_scroll_saturating() {
        let mut manager = PlacementManager::new();

        manager.add(make_placement(1, 1, 2, 10));

        manager.scroll(-10);

        let p = manager.get(1, 1).unwrap();
        assert_eq!(p.row, 0);
    }

    // =========================================================================
    // Utility
    // =========================================================================

    #[test]
    fn test_image_ids() {
        let mut manager = PlacementManager::new();

        manager.add(make_placement(1, 1, 5, 10));
        manager.add(make_placement(1, 2, 15, 20));
        manager.add(make_placement(2, 1, 25, 30));
        manager.add(make_placement(3, 1, 35, 40));

        let ids = manager.image_ids();
        assert_eq!(ids.len(), 3);
        assert!(ids.contains(&1));
        assert!(ids.contains(&2));
        assert!(ids.contains(&3));
    }
}
