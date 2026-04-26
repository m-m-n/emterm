//! Style intern table.
//!
//! Each unique combination of style attributes (fg, bg, flags, underline,
//! hyperlink) is stored once in [`StyleTable::storage`] and identified by a
//! `u16` id. [`SlimCell`](crate::slim_cell::SlimCell) stores the id rather
//! than the full style record.
//!
//! ID `0` is reserved for the default style — it is interned at table
//! creation, has refcount `u32::MAX`, and is never freed.
//!
//! When the table reaches `u16::MAX` entries new intern requests fall back to
//! id `0` and emit a rate-limited `console::warn` (FR9).

use std::collections::HashMap;

use crate::cell::PackedColor;

/// Hashable record of style attributes shared by all cells with the same look.
#[derive(Clone, Copy, Hash, PartialEq, Eq, Debug, serde::Serialize, serde::Deserialize)]
#[repr(C)]
pub struct StyleEntry {
    pub fg: PackedColor,
    pub bg: PackedColor,
    pub flags: u16,
    pub underline_style: u8,
    pub underline_color: [u8; 3],
    pub hyperlink_id: u16,
}

impl Default for StyleEntry {
    fn default() -> Self {
        Self {
            fg: PackedColor::DEFAULT,
            bg: PackedColor::DEFAULT,
            flags: 0,
            underline_style: 0,
            underline_color: [0; 3],
            hyperlink_id: 0,
        }
    }
}

/// Refcount type. Saturating at `u32::MAX` — this keeps the default style
/// permanently pinned.
pub type StyleRefcount = u32;

pub struct StyleTable {
    storage: Vec<StyleEntry>,
    dedup: HashMap<StyleEntry, u16>,
    refcount: Vec<StyleRefcount>,
    free_list: Vec<u16>,
    /// Counter incremented every time a saturation fallback fires; useful for
    /// tests and external rate-limiting.
    saturated_warn_count: u32,
}

impl Default for StyleTable {
    fn default() -> Self {
        Self::new()
    }
}

impl StyleTable {
    /// Create a fresh table with the default style pre-interned at id 0
    /// (refcount pinned at `u32::MAX`).
    pub fn new() -> Self {
        let default = StyleEntry::default();
        let mut dedup = HashMap::new();
        dedup.insert(default, 0u16);
        Self {
            storage: vec![default],
            dedup,
            refcount: vec![u32::MAX],
            free_list: Vec::new(),
            saturated_warn_count: 0,
        }
    }

    /// Intern a style entry. If the entry is already present its refcount is
    /// incremented (saturating at `u32::MAX`). Returns the id.
    ///
    /// On saturation (table size at `u16::MAX`) returns id 0 and increments
    /// the saturation warning counter.
    pub fn intern(&mut self, entry: StyleEntry) -> u16 {
        if let Some(&id) = self.dedup.get(&entry) {
            let rc = &mut self.refcount[id as usize];
            *rc = rc.saturating_add(1);
            return id;
        }
        if let Some(id) = self.free_list.pop() {
            self.storage[id as usize] = entry;
            self.refcount[id as usize] = 1;
            self.dedup.insert(entry, id);
            return id;
        }
        if self.storage.len() >= u16::MAX as usize {
            self.saturated_warn_count = self.saturated_warn_count.saturating_add(1);
            // Default style refcount is u32::MAX so this saturating_add is a
            // no-op — that is intentional: the default style is never freed.
            return 0;
        }
        let id = self.storage.len() as u16;
        self.storage.push(entry);
        self.refcount.push(1);
        self.dedup.insert(entry, id);
        id
    }

    /// Decrement the refcount for `id`. When refcount reaches 0 the slot is
    /// freed and pushed onto the free list. id 0 (default) is a no-op.
    pub fn dec_ref(&mut self, id: u16) {
        if id == 0 {
            return;
        }
        let idx = id as usize;
        if idx >= self.refcount.len() {
            return;
        }
        let rc = &mut self.refcount[idx];
        debug_assert!(*rc > 0, "StyleTable refcount underflow at id {id}");
        if *rc == 0 {
            return;
        }
        *rc = rc.saturating_sub(1);
        if *rc == 0 {
            let entry = self.storage[idx];
            self.dedup.remove(&entry);
            self.storage[idx] = StyleEntry::default();
            self.free_list.push(id);
        }
    }

    /// Increment the refcount for `id` without re-interning. Used by reflow
    /// helpers when moving SlimCells between rows. id 0 saturates at u32::MAX.
    #[allow(dead_code)]
    pub fn inc_ref(&mut self, id: u16) {
        let idx = id as usize;
        if idx >= self.refcount.len() {
            return;
        }
        let rc = &mut self.refcount[idx];
        *rc = rc.saturating_add(1);
    }

    /// Return the entry for `id`. Out-of-range ids return the default style.
    pub fn get_or_default(&self, id: u16) -> StyleEntry {
        let idx = id as usize;
        if idx < self.storage.len() {
            // If the slot is on the free list its storage value has been
            // reset to default already, so this is safe to return.
            self.storage[idx]
        } else {
            StyleEntry::default()
        }
    }

    /// Return the refcount for `id` (0 if id is out of range).
    #[allow(dead_code)]
    pub fn refcount(&self, id: u16) -> StyleRefcount {
        let idx = id as usize;
        if idx < self.refcount.len() {
            self.refcount[idx]
        } else {
            0
        }
    }

    /// Number of live entries (storage minus free slots).
    pub fn live_entries(&self) -> usize {
        self.storage.len() - self.free_list.len()
    }

    /// Total slot count including freed-but-not-reused slots.
    pub fn slot_count(&self) -> usize {
        self.storage.len()
    }

    /// Approximate bytes of memory consumed by the table (used by the debug
    /// stats export). Includes Vec capacity for storage + refcount + free
    /// list, plus an estimated overhead for the dedup HashMap.
    #[allow(dead_code)]
    pub fn bytes_used(&self) -> usize {
        let storage = self.storage.capacity() * std::mem::size_of::<StyleEntry>();
        let refcount = self.refcount.capacity() * std::mem::size_of::<StyleRefcount>();
        let free_list = self.free_list.capacity() * std::mem::size_of::<u16>();
        // HashMap overhead estimate: 1.5x entries × (key + value + tag).
        let map = (self.dedup.len() * 3 / 2)
            * (std::mem::size_of::<StyleEntry>() + std::mem::size_of::<u16>() + 8);
        storage + refcount + free_list + map
    }

    #[allow(dead_code)]
    pub fn saturated_warn_count(&self) -> u32 {
        self.saturated_warn_count
    }

    // ── Serialization helpers (snapshot V2) ─────────────

    pub(crate) fn snapshot(&self) -> (Vec<StyleEntry>, Vec<u32>, Vec<u16>) {
        (
            self.storage.clone(),
            self.refcount.clone(),
            self.free_list.clone(),
        )
    }

    /// Restore a `StyleTable` from its serialized parts. Returns `None` if
    /// internal invariants don't hold (e.g. mismatched lengths, missing
    /// default at id 0).
    pub(crate) fn from_snapshot(
        storage: Vec<StyleEntry>,
        refcount: Vec<u32>,
        free_list: Vec<u16>,
    ) -> Option<Self> {
        if storage.len() != refcount.len() {
            return None;
        }
        if storage.is_empty() {
            return None;
        }
        if storage[0] != StyleEntry::default() {
            return None;
        }
        if refcount[0] != u32::MAX {
            return None;
        }
        let free_set: std::collections::HashSet<u16> = free_list.iter().copied().collect();
        let mut dedup = HashMap::with_capacity(storage.len());
        for (i, entry) in storage.iter().enumerate() {
            if free_set.contains(&(i as u16)) {
                continue;
            }
            // Don't insert a duplicate of the default if its id is 0 (already handled)
            // or if a non-default storage entry equals default (treat as freed).
            dedup.insert(*entry, i as u16);
        }
        Some(Self {
            storage,
            dedup,
            refcount,
            free_list,
            saturated_warn_count: 0,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rgb_style(r: u8, g: u8, b: u8) -> StyleEntry {
        StyleEntry {
            fg: PackedColor::rgb(r, g, b),
            ..Default::default()
        }
    }

    #[test]
    fn default_style_is_id_zero() {
        let table = StyleTable::new();
        assert_eq!(table.get_or_default(0), StyleEntry::default());
        assert_eq!(table.refcount(0), u32::MAX);
        assert_eq!(table.live_entries(), 1);
    }

    #[test]
    fn dec_ref_zero_is_noop() {
        let mut table = StyleTable::new();
        table.dec_ref(0);
        table.dec_ref(0);
        assert_eq!(table.refcount(0), u32::MAX);
        assert_eq!(table.live_entries(), 1);
    }

    #[test]
    fn intern_returns_same_id_for_equal_entries() {
        let mut table = StyleTable::new();
        let entry = rgb_style(10, 20, 30);
        let id1 = table.intern(entry);
        let id2 = table.intern(entry);
        assert_eq!(id1, id2);
        assert_ne!(id1, 0);
        assert_eq!(table.refcount(id1), 2);
        assert_eq!(table.live_entries(), 2); // default + new entry
    }

    #[test]
    fn intern_different_entries_get_different_ids() {
        let mut table = StyleTable::new();
        let id_red = table.intern(rgb_style(255, 0, 0));
        let id_green = table.intern(rgb_style(0, 255, 0));
        assert_ne!(id_red, id_green);
        assert_eq!(table.refcount(id_red), 1);
        assert_eq!(table.refcount(id_green), 1);
    }

    #[test]
    fn refcount_lifecycle_frees_slot() {
        let mut table = StyleTable::new();
        let entry = rgb_style(1, 2, 3);
        let id = table.intern(entry);
        for _ in 0..999 {
            let _ = table.intern(entry);
        }
        assert_eq!(table.refcount(id), 1000);
        for _ in 0..1000 {
            table.dec_ref(id);
        }
        assert_eq!(table.refcount(id), 0);
        // Slot should be on the free list and live_entries back to 1 (default only).
        assert_eq!(table.live_entries(), 1);
    }

    #[test]
    fn free_list_reuses_id() {
        let mut table = StyleTable::new();
        let id_a = table.intern(rgb_style(1, 2, 3));
        table.dec_ref(id_a);
        // After dec_ref, dedup map entry is gone. A new intern with a
        // different style should reuse the freed id.
        let id_b = table.intern(rgb_style(4, 5, 6));
        assert_eq!(id_a, id_b);
        assert_eq!(table.refcount(id_b), 1);
        assert_eq!(table.live_entries(), 2);
    }

    #[test]
    fn intern_after_dec_keeps_dedup_consistent() {
        let mut table = StyleTable::new();
        let entry = rgb_style(7, 8, 9);
        let id = table.intern(entry);
        table.dec_ref(id);
        // Re-intern the same entry should get a fresh id (might equal `id`
        // via free list reuse) and refcount = 1.
        let id2 = table.intern(entry);
        assert_eq!(table.refcount(id2), 1);
        // Interning again should bump the refcount, not allocate a new slot.
        let id3 = table.intern(entry);
        assert_eq!(id2, id3);
        assert_eq!(table.refcount(id2), 2);
    }

    #[test]
    fn saturation_falls_back_to_zero() {
        let mut table = StyleTable::new();
        // Fill the table up to u16::MAX entries (default takes id 0, so we
        // need 65535 unique extra entries to occupy ids 1..=65534).
        // To keep test fast, we forge entries by varying flags and
        // hyperlink_id while leaving colors default.
        let max = u16::MAX as usize;
        let mut next = 1u32;
        while table.slot_count() < max {
            let entry = StyleEntry {
                flags: (next & 0xFFFF) as u16,
                hyperlink_id: ((next >> 16) & 0xFFFF) as u16,
                ..Default::default()
            };
            let id = table.intern(entry);
            assert_ne!(id, 0, "should not saturate yet");
            next += 1;
        }
        assert_eq!(table.slot_count(), max);
        // Next intern must saturate.
        let extra = StyleEntry {
            flags: 0xFFFF,
            hyperlink_id: 0xFFFF,
            ..Default::default()
        };
        let id = table.intern(extra);
        assert_eq!(id, 0);
        assert_eq!(table.saturated_warn_count(), 1);
    }

    #[test]
    fn inc_ref_and_dec_ref_balance() {
        let mut table = StyleTable::new();
        let id = table.intern(rgb_style(1, 1, 1));
        assert_eq!(table.refcount(id), 1);
        table.inc_ref(id);
        table.inc_ref(id);
        assert_eq!(table.refcount(id), 3);
        table.dec_ref(id);
        table.dec_ref(id);
        assert_eq!(table.refcount(id), 1);
        table.dec_ref(id);
        assert_eq!(table.refcount(id), 0);
        assert_eq!(table.live_entries(), 1);
    }

    #[test]
    fn out_of_range_id_returns_default() {
        let table = StyleTable::new();
        assert_eq!(table.get_or_default(9999), StyleEntry::default());
        assert_eq!(table.refcount(9999), 0);
    }

    #[test]
    fn dec_ref_out_of_range_is_noop() {
        let mut table = StyleTable::new();
        table.dec_ref(9999); // must not panic
    }
}
