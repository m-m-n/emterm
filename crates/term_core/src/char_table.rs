//! Grapheme intern table.
//!
//! [`SlimCell`](crate::slim_cell::SlimCell) instances whose grapheme exceeds
//! 4 bytes UTF-8 (so it cannot be packed into the `char_ref` field) reference
//! a `CharTable` entry by `u32` id.
//!
//! The table is reference-counted with a free list so that scrollback cells
//! that age out of the ring release their entries cleanly.

use std::collections::HashMap;

pub type CharRefcount = u32;

/// Sentinel string used for out-of-range or freed entries.
const DEFAULT_STR: &str = "?";

/// Sentinel id returned by `intern` on saturation; `get_or_default` resolves
/// it (and any other out-of-range id) to `DEFAULT_STR`.
pub const CHAR_TABLE_SATURATED_ID: u32 = u32::MAX;

/// Practical upper bound on the number of intern slots. The format reserves
/// `u32::MAX` as the saturation sentinel, so usable ids are `0..MAX_ENTRIES`.
const MAX_ENTRIES: usize = (u32::MAX - 1) as usize;

pub struct CharTable {
    storage: Vec<String>,
    dedup: HashMap<String, u32>,
    refcount: Vec<CharRefcount>,
    free_list: Vec<u32>,
    /// Counter incremented every time a saturation fallback fires.
    saturated_warn_count: u32,
}

impl Default for CharTable {
    fn default() -> Self {
        Self::new()
    }
}

impl CharTable {
    pub fn new() -> Self {
        Self {
            storage: Vec::new(),
            dedup: HashMap::new(),
            refcount: Vec::new(),
            free_list: Vec::new(),
            saturated_warn_count: 0,
        }
    }

    /// Intern `s`. Returns the id; refcount incremented (saturating).
    ///
    /// On saturation (table size at `MAX_ENTRIES`) returns
    /// [`CHAR_TABLE_SATURATED_ID`] (which `get_or_default` resolves to `"?"`)
    /// and increments the saturation warning counter.
    pub fn intern(&mut self, s: &str) -> u32 {
        if let Some(&id) = self.dedup.get(s) {
            let rc = &mut self.refcount[id as usize];
            *rc = rc.saturating_add(1);
            return id;
        }
        if let Some(id) = self.free_list.pop() {
            self.storage[id as usize] = s.to_owned();
            self.refcount[id as usize] = 1;
            self.dedup.insert(s.to_owned(), id);
            return id;
        }
        if self.storage.len() >= MAX_ENTRIES {
            self.saturated_warn_count = self.saturated_warn_count.saturating_add(1);
            return CHAR_TABLE_SATURATED_ID;
        }
        let id = self.storage.len() as u32;
        self.storage.push(s.to_owned());
        self.refcount.push(1);
        self.dedup.insert(s.to_owned(), id);
        id
    }

    #[allow(dead_code)]
    pub fn saturated_warn_count(&self) -> u32 {
        self.saturated_warn_count
    }

    pub fn dec_ref(&mut self, id: u32) {
        let idx = id as usize;
        if idx >= self.refcount.len() {
            return;
        }
        let rc = &mut self.refcount[idx];
        debug_assert!(*rc > 0, "CharTable refcount underflow at id {id}");
        if *rc == 0 {
            return;
        }
        *rc = rc.saturating_sub(1);
        if *rc == 0 {
            let s = std::mem::take(&mut self.storage[idx]);
            self.dedup.remove(&s);
            self.free_list.push(id);
        }
    }

    #[allow(dead_code)]
    pub fn inc_ref(&mut self, id: u32) {
        let idx = id as usize;
        if idx >= self.refcount.len() {
            return;
        }
        let rc = &mut self.refcount[idx];
        *rc = rc.saturating_add(1);
    }

    /// Return the string for `id`, or a sentinel if `id` is out of range or
    /// the slot has been freed.
    pub fn get_or_default(&self, id: u32) -> &str {
        let idx = id as usize;
        if idx < self.storage.len() {
            let s = &self.storage[idx];
            if s.is_empty() {
                DEFAULT_STR
            } else {
                s.as_str()
            }
        } else {
            DEFAULT_STR
        }
    }

    #[allow(dead_code)]
    pub fn refcount(&self, id: u32) -> CharRefcount {
        let idx = id as usize;
        if idx < self.refcount.len() {
            self.refcount[idx]
        } else {
            0
        }
    }

    pub fn live_entries(&self) -> usize {
        self.storage.len() - self.free_list.len()
    }

    #[allow(dead_code)]
    pub fn slot_count(&self) -> usize {
        self.storage.len()
    }

    pub(crate) fn snapshot(&self) -> (Vec<String>, Vec<u32>, Vec<u32>) {
        (
            self.storage.clone(),
            self.refcount.clone(),
            self.free_list.clone(),
        )
    }

    pub(crate) fn from_snapshot(
        storage: Vec<String>,
        refcount: Vec<u32>,
        free_list: Vec<u32>,
    ) -> Option<Self> {
        if storage.len() != refcount.len() {
            return None;
        }
        let free_set: std::collections::HashSet<u32> = free_list.iter().copied().collect();
        let mut dedup = HashMap::with_capacity(storage.len());
        for (i, s) in storage.iter().enumerate() {
            if free_set.contains(&(i as u32)) {
                continue;
            }
            if s.is_empty() {
                continue;
            }
            dedup.insert(s.clone(), i as u32);
        }
        Some(Self {
            storage,
            dedup,
            refcount,
            free_list,
            saturated_warn_count: 0,
        })
    }

    /// Approximate bytes consumed (storage strings + Vec capacity + dedup map).
    #[allow(dead_code)]
    pub fn bytes_used(&self) -> usize {
        let storage_strings: usize = self.storage.iter().map(|s| s.capacity()).sum();
        let storage_vec = self.storage.capacity() * std::mem::size_of::<String>();
        let refcount_vec = self.refcount.capacity() * std::mem::size_of::<CharRefcount>();
        let free_list_vec = self.free_list.capacity() * std::mem::size_of::<u32>();
        let map_strings: usize = self.dedup.keys().map(|s| s.capacity()).sum();
        let map_overhead = (self.dedup.len() * 3 / 2)
            * (std::mem::size_of::<String>() + std::mem::size_of::<u32>() + 8);
        storage_strings + storage_vec + refcount_vec + free_list_vec + map_strings + map_overhead
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn intern_returns_same_id_for_equal_strings() {
        let mut table = CharTable::new();
        let id1 = table.intern("👨‍👩‍👧‍👦");
        let id2 = table.intern("👨‍👩‍👧‍👦");
        assert_eq!(id1, id2);
        assert_eq!(table.refcount(id1), 2);
        assert_eq!(table.live_entries(), 1);
    }

    #[test]
    fn intern_different_strings_get_different_ids() {
        let mut table = CharTable::new();
        let a = table.intern("🇯🇵");
        let b = table.intern("🇺🇸");
        assert_ne!(a, b);
        assert_eq!(table.refcount(a), 1);
        assert_eq!(table.refcount(b), 1);
    }

    #[test]
    fn dec_ref_to_zero_frees_slot() {
        let mut table = CharTable::new();
        let id = table.intern("hello");
        table.dec_ref(id);
        assert_eq!(table.refcount(id), 0);
        assert_eq!(table.live_entries(), 0);
        // The dedup entry must be gone — re-interning the same string gets a
        // fresh insertion (potentially reusing the freed id).
        let id2 = table.intern("hello");
        assert_eq!(id, id2, "freed id should be reused");
        assert_eq!(table.refcount(id2), 1);
    }

    #[test]
    fn refcount_lifecycle() {
        let mut table = CharTable::new();
        let id = table.intern("x");
        for _ in 0..999 {
            let _ = table.intern("x");
        }
        assert_eq!(table.refcount(id), 1000);
        for _ in 0..1000 {
            table.dec_ref(id);
        }
        assert_eq!(table.refcount(id), 0);
        assert_eq!(table.live_entries(), 0);
    }

    #[test]
    fn free_list_reuses_id() {
        let mut table = CharTable::new();
        let id_a = table.intern("a");
        table.dec_ref(id_a);
        let id_b = table.intern("b");
        assert_eq!(id_a, id_b, "freed id reused");
    }

    #[test]
    fn get_or_default_returns_sentinel_for_freed() {
        let mut table = CharTable::new();
        let id = table.intern("kept");
        let id2 = table.intern("dropped");
        table.dec_ref(id2);
        assert_eq!(table.get_or_default(id), "kept");
        assert_eq!(table.get_or_default(id2), "?");
    }

    #[test]
    fn get_or_default_out_of_range() {
        let table = CharTable::new();
        assert_eq!(table.get_or_default(0), "?");
        assert_eq!(table.get_or_default(9999), "?");
    }

    #[test]
    fn inc_ref_and_dec_ref_balance() {
        let mut table = CharTable::new();
        let id = table.intern("z");
        table.inc_ref(id);
        table.inc_ref(id);
        assert_eq!(table.refcount(id), 3);
        table.dec_ref(id);
        table.dec_ref(id);
        assert_eq!(table.refcount(id), 1);
        table.dec_ref(id);
        assert_eq!(table.refcount(id), 0);
    }

    #[test]
    fn dec_ref_out_of_range_is_noop() {
        let mut table = CharTable::new();
        table.dec_ref(9999); // must not panic
    }
}
