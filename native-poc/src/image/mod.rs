//! Native-poc inline-image overlay layer.
//!
//! This module implements the Kitty Graphics Protocol + SIXEL surface for
//! the native build. It owns:
//!
//! - A pure-Rust **state** machine ([`ImageLayerState`]) that tracks which
//!   image bytes the renderer should be aware of (image id → byte size,
//!   placement ids, LRU order, memory budget). The state machine is
//!   entirely GPU-free so it can be exhaustively unit-tested without a
//!   wgpu device.
//! - A GPU-coupled [`ImageLayer`] wrapper that owns the actual
//!   `wgpu::Texture` handles and the per-placement bind groups. It is a
//!   thin façade over the state machine plus a hash map of textures.
//! - The [`recompute_pixel_dims`] helper, called on window resize so
//!   image placements (anchored by `(row, col)`) re-derive pixel
//!   positions from the new cell metrics.
//!
//! The actual wgpu draw pipeline lives in [`overlay`]. Adapter logic
//! between APC/DCS byte payloads and `term_images::ImageProcessor`
//! lives in [`parse`].

pub mod overlay;
pub mod parse;

use std::collections::{BTreeMap, HashMap, VecDeque};

use term_images::image_proc::{DecodedImage, ImageDelete, ImageEvent, ImagePlacement};

/// Default per-tab image-memory quota (matches `Settings::image_memory_quota_mb`
/// default of 320 MB). Independent constant so unit tests don't need to
/// import the entire `Settings` struct.
#[allow(dead_code)] // Settings provides the runtime value; this is the unit-test/fallback default.
pub const DEFAULT_IMAGE_QUOTA_BYTES: u64 = 320 * 1024 * 1024;

/// A placement enriched with per-frame pixel coordinates derived from the
/// current cell metrics. The render pipeline consumes these.
#[derive(Debug, Clone)]
pub struct PixelPlacement {
    pub image_id: u32,
    pub placement_id: u32,
    pub z_index: i32,
    /// Top-left in physical pixels (already includes any cell offsets).
    pub pixel_x: u32,
    pub pixel_y: u32,
    pub pixel_w: u32,
    pub pixel_h: u32,
}

/// Pure (GPU-free) half of the image overlay. Tracks image byte sizes,
/// placements, LRU order, and memory budget. The render side ([`ImageLayer`])
/// owns the actual `wgpu::Texture` handles in a parallel map keyed by the
/// same `image_id`.
///
/// Invariants:
/// - `memory_used` is always equal to `sum(image_sizes.values())`.
/// - `lru` contains every key in `image_sizes` exactly once. Front = LRU
///   (next eviction target), back = MRU (most recently inserted /
///   re-touched).
#[derive(Debug)]
pub struct ImageLayerState {
    /// Image byte sizes, keyed by image id.
    image_sizes: HashMap<u32, u64>,
    /// Placements, keyed by (z_index, image_id, placement_id) so iteration
    /// order is back-to-front (negative z → behind text → drawn first).
    placements: BTreeMap<(i32, u32, u32), ImagePlacement>,
    /// LRU order: front = least recently inserted, back = most recently
    /// inserted. Used by [`evict_until_quota`] to choose victims.
    lru: VecDeque<u32>,
    /// Total bytes currently owned by `image_sizes`.
    memory_used: u64,
    /// Hard cap. When `memory_used` exceeds this after an insert, the LRU
    /// front is evicted until the cap is met.
    quota_bytes: u64,
    /// Current cell metrics in physical pixels. Updated by
    /// [`recompute_pixel_dims`]. Defaults to a sensible (8, 16) so unit
    /// tests don't have to initialize this for every case.
    cell_w: u32,
    cell_h: u32,
}

impl ImageLayerState {
    pub fn new(quota_bytes: u64) -> Self {
        Self {
            image_sizes: HashMap::new(),
            placements: BTreeMap::new(),
            lru: VecDeque::new(),
            memory_used: 0,
            quota_bytes,
            cell_w: 8,
            cell_h: 16,
        }
    }

    pub fn quota_bytes(&self) -> u64 {
        self.quota_bytes
    }
    pub fn memory_used(&self) -> u64 {
        self.memory_used
    }
    pub fn image_count(&self) -> usize {
        self.image_sizes.len()
    }
    pub fn placement_count(&self) -> usize {
        self.placements.len()
    }
    pub fn cell_w(&self) -> u32 {
        self.cell_w
    }
    pub fn cell_h(&self) -> u32 {
        self.cell_h
    }

    /// Returns the IDs of all currently-known images, in arbitrary order.
    /// Used by [`ImageLayer`] to find textures it can drop after eviction.
    pub fn image_ids(&self) -> Vec<u32> {
        self.image_sizes.keys().copied().collect()
    }

    /// Returns the placements in back-to-front order (lowest `z_index`
    /// first). The renderer iterates this directly.
    pub fn placements(&self) -> impl Iterator<Item = &ImagePlacement> {
        self.placements.values()
    }

    /// Record (or update) an image's byte size, mark it most-recently-used,
    /// and return the IDs that were evicted to stay under quota (if any).
    pub fn record_image(&mut self, image_id: u32, byte_size: u64) -> Vec<u32> {
        if let Some(prev) = self.image_sizes.insert(image_id, byte_size) {
            self.memory_used = self.memory_used.saturating_sub(prev);
            // Remove existing LRU entry; we'll push it to the back below.
            if let Some(pos) = self.lru.iter().position(|id| *id == image_id) {
                self.lru.remove(pos);
            }
        }
        self.memory_used = self.memory_used.saturating_add(byte_size);
        self.lru.push_back(image_id);
        self.evict_until_quota()
    }

    /// Touch an existing image so it moves to the MRU end of the LRU.
    /// No-op if the image is unknown. Used when a Place event references
    /// an already-stored image.
    pub fn touch_image(&mut self, image_id: u32) {
        if let Some(pos) = self.lru.iter().position(|id| *id == image_id) {
            self.lru.remove(pos);
            self.lru.push_back(image_id);
        }
    }

    /// Drop an image and all its placements. Returns the byte size that
    /// was freed (0 if the image was not present).
    pub fn drop_image(&mut self, image_id: u32) -> u64 {
        let freed = self.image_sizes.remove(&image_id).unwrap_or(0);
        if freed > 0 {
            self.memory_used = self.memory_used.saturating_sub(freed);
            if let Some(pos) = self.lru.iter().position(|id| *id == image_id) {
                self.lru.remove(pos);
            }
            // Drop placements that referenced this image.
            self.placements.retain(|(_, iid, _), _| *iid != image_id);
        }
        freed
    }

    /// Insert (or replace) a placement.
    pub fn insert_placement(&mut self, placement: ImagePlacement) {
        let key = (
            placement.z_index,
            placement.image_id,
            placement.placement_id,
        );
        self.placements.insert(key, placement);
    }

    /// Apply a delete spec from `term_images`.
    pub fn apply_delete(&mut self, target: &ImageDelete) {
        match target {
            ImageDelete::All | ImageDelete::AllIncludingHidden => {
                self.placements.clear();
            }
            ImageDelete::ById(image_id) => {
                self.placements.retain(|(_, iid, _), _| *iid != *image_id);
            }
            ImageDelete::ByPlacement {
                image_id,
                placement_id,
            } => {
                self.placements
                    .retain(|(_, iid, pid), _| !(*iid == *image_id && *pid == *placement_id));
            }
            ImageDelete::AtCursor { row, col } => {
                self.placements
                    .retain(|_, p| !(p.row == *row && p.col == *col));
            }
            ImageDelete::ByZIndex(z) => {
                self.placements.retain(|(zk, _, _), _| zk != z);
            }
        }
    }

    /// Evict images from the LRU front until `memory_used <= quota_bytes`.
    /// Returns the evicted image IDs. Also clears any placements that
    /// referenced those images.
    pub fn evict_until_quota(&mut self) -> Vec<u32> {
        let mut evicted = Vec::new();
        while self.memory_used > self.quota_bytes {
            let victim = match self.lru.pop_front() {
                Some(v) => v,
                None => break,
            };
            if let Some(sz) = self.image_sizes.remove(&victim) {
                self.memory_used = self.memory_used.saturating_sub(sz);
                self.placements.retain(|(_, iid, _), _| *iid != victim);
                evicted.push(victim);
                log::warn!(
                    "image quota: evicted image id={} ({} bytes, now {}/{} used)",
                    victim,
                    sz,
                    self.memory_used,
                    self.quota_bytes
                );
            }
        }
        evicted
    }

    /// Update cell metrics. The renderer reads `cell_w` / `cell_h` to
    /// derive per-placement pixel coordinates on every frame.
    pub fn recompute_pixel_dims(&mut self, cell_w: u32, cell_h: u32) {
        self.cell_w = cell_w.max(1);
        self.cell_h = cell_h.max(1);
    }

    /// Resolve every placement to its pixel rectangle using the current
    /// cell metrics. Width/height fall back to the stored image's pixel
    /// size when `columns`/`rows` are unset (Kitty: 0 = auto).
    pub fn resolve_pixel_placements<F>(&self, image_dims: F) -> Vec<PixelPlacement>
    where
        F: Fn(u32) -> Option<(u32, u32)>,
    {
        let mut out = Vec::with_capacity(self.placements.len());
        for ((z, image_id, placement_id), p) in &self.placements {
            let pixel_x = p.col.saturating_mul(self.cell_w).saturating_add(p.x_offset);
            let pixel_y = p.row.saturating_mul(self.cell_h).saturating_add(p.y_offset);
            let (pixel_w, pixel_h) = if p.columns > 0 && p.rows > 0 {
                (
                    p.columns.saturating_mul(self.cell_w),
                    p.rows.saturating_mul(self.cell_h),
                )
            } else if let Some((iw, ih)) = image_dims(*image_id) {
                (iw, ih)
            } else {
                (self.cell_w, self.cell_h) // Last-resort sentinel.
            };
            out.push(PixelPlacement {
                image_id: *image_id,
                placement_id: *placement_id,
                z_index: *z,
                pixel_x,
                pixel_y,
                pixel_w,
                pixel_h,
            });
        }
        out
    }
}

/// Split a stream of [`ImageEvent`]s produced by [`parse::decode_apc`] /
/// [`parse::decode_dcs`] into:
/// 1. the events the layer state should ingest (ImageReady, Place, Delete),
/// 2. the response bytes the caller must echo back to the PTY (Kitty OK
///    replies, query responses, …).
///
/// Animation and QueryResponse variants are forwarded to the state-affecting
/// stream and currently ignored — Phase 5 does not animate.
pub fn split_image_events(events: Vec<ImageEvent>) -> (Vec<ImageEvent>, Vec<String>) {
    let mut state = Vec::with_capacity(events.len());
    let mut responses = Vec::new();
    for e in events {
        match e {
            ImageEvent::Response { data } => responses.push(data),
            other => state.push(other),
        }
    }
    (state, responses)
}

/// Apply a stream of (already-split) events to the state machine. Returns
/// any image IDs that were evicted as a side-effect, so the caller can
/// drop matching GPU textures.
pub fn ingest_state_events(
    state: &mut ImageLayerState,
    events: Vec<ImageEvent>,
    decoded_images: &mut HashMap<u32, DecodedImage>,
) -> Vec<u32> {
    let mut evicted_total: Vec<u32> = Vec::new();
    for evt in events {
        match evt {
            ImageEvent::ImageReady { image } => {
                let id = image.id;
                let byte_size = image.rgba_data.len() as u64;
                decoded_images.insert(id, image);
                let evicted = state.record_image(id, byte_size);
                for v in &evicted {
                    decoded_images.remove(v);
                }
                evicted_total.extend(evicted);
            }
            ImageEvent::Place { placement } => {
                state.touch_image(placement.image_id);
                state.insert_placement(placement);
            }
            ImageEvent::Delete { target } => {
                if matches!(target, ImageDelete::All | ImageDelete::AllIncludingHidden) {
                    // Per Kitty spec these targets remove placements only;
                    // the image bytes stay so they can be re-Placed. The
                    // legacy WebView build behaves this way too.
                }
                state.apply_delete(&target);
            }
            ImageEvent::QueryResponse { .. } => {
                // Query is a probe; the protocol response goes through
                // ImageEvent::Response which we already split off above.
            }
            ImageEvent::Animation(_) => {
                // Phase 5 does not animate. Tracked for future work.
                log::debug!("image animation event ignored (Phase 5 scope)");
            }
            ImageEvent::Response { .. } => {
                debug_assert!(
                    false,
                    "Response events must be split off before ingest_state_events"
                );
            }
        }
    }
    evicted_total
}

#[cfg(test)]
mod tests {
    use super::*;
    use term_images::image_proc::DecodedImage;

    fn placement(image_id: u32, placement_id: u32, row: u32, col: u32, z: i32) -> ImagePlacement {
        ImagePlacement {
            image_id,
            placement_id,
            row,
            col,
            z_index: z,
            ..ImagePlacement::default()
        }
    }

    fn decoded(id: u32, byte_size: usize) -> DecodedImage {
        DecodedImage {
            id,
            width: 4,
            height: byte_size as u32 / 16, // 4 bytes/pixel × 4 px wide
            rgba_data: vec![0u8; byte_size],
            rgba_base64: String::new(),
        }
    }

    #[test]
    fn state_new_starts_empty() {
        let s = ImageLayerState::new(1024);
        assert_eq!(s.image_count(), 0);
        assert_eq!(s.placement_count(), 0);
        assert_eq!(s.memory_used(), 0);
        assert_eq!(s.quota_bytes(), 1024);
    }

    #[test]
    fn record_image_increments_memory() {
        let mut s = ImageLayerState::new(1024);
        let evicted = s.record_image(1, 100);
        assert_eq!(s.memory_used(), 100);
        assert_eq!(s.image_count(), 1);
        assert!(evicted.is_empty());
    }

    #[test]
    fn record_image_replaces_size_when_same_id() {
        let mut s = ImageLayerState::new(1024);
        s.record_image(1, 100);
        s.record_image(1, 250);
        assert_eq!(s.memory_used(), 250);
        assert_eq!(s.image_count(), 1);
    }

    #[test]
    fn record_image_evicts_lru_when_over_quota() {
        let mut s = ImageLayerState::new(300);
        s.record_image(1, 100);
        s.record_image(2, 100);
        s.record_image(3, 100);
        // At quota, nothing evicted yet.
        assert_eq!(s.memory_used(), 300);
        let evicted = s.record_image(4, 100);
        assert_eq!(evicted, vec![1]);
        assert_eq!(s.memory_used(), 300);
        assert!(!s.image_ids().contains(&1));
        assert!(s.image_ids().contains(&4));
    }

    #[test]
    fn record_image_evicts_multiple_when_huge_insert() {
        let mut s = ImageLayerState::new(300);
        s.record_image(1, 100);
        s.record_image(2, 100);
        s.record_image(3, 100);
        let evicted = s.record_image(4, 250);
        // Need to free until ≤300; after insert mem=550 → evict 1 (450)
        // → evict 2 (350) → evict 3 (250). Only image 4 remains.
        assert_eq!(evicted, vec![1, 2, 3]);
        assert_eq!(s.image_count(), 1);
        assert_eq!(s.memory_used(), 250);
    }

    #[test]
    fn touch_image_moves_to_mru_end() {
        let mut s = ImageLayerState::new(300);
        s.record_image(1, 100);
        s.record_image(2, 100);
        s.record_image(3, 100);
        s.touch_image(1); // 1 is now MRU
        let evicted = s.record_image(4, 100);
        // 2 should be evicted now (was the LRU front after touch).
        assert_eq!(evicted, vec![2]);
        assert!(s.image_ids().contains(&1));
    }

    #[test]
    fn drop_image_frees_memory_and_placements() {
        let mut s = ImageLayerState::new(1024);
        s.record_image(1, 100);
        s.insert_placement(placement(1, 1, 0, 0, -1));
        s.insert_placement(placement(2, 1, 0, 0, -1));
        assert_eq!(s.placement_count(), 2);
        let freed = s.drop_image(1);
        assert_eq!(freed, 100);
        assert_eq!(s.memory_used(), 0);
        // Placement for image 1 is gone; image 2's placement stays.
        assert_eq!(s.placement_count(), 1);
    }

    #[test]
    fn drop_image_unknown_id_is_noop() {
        let mut s = ImageLayerState::new(1024);
        s.record_image(1, 100);
        let freed = s.drop_image(99);
        assert_eq!(freed, 0);
        assert_eq!(s.memory_used(), 100);
    }

    #[test]
    fn placements_iterate_back_to_front_by_z() {
        let mut s = ImageLayerState::new(1024);
        // Insert in random order; BTreeMap keys order on z_index.
        s.insert_placement(placement(1, 1, 0, 0, 5));
        s.insert_placement(placement(2, 1, 0, 0, -5));
        s.insert_placement(placement(3, 1, 0, 0, 0));
        let zs: Vec<i32> = s.placements().map(|p| p.z_index).collect();
        assert_eq!(zs, vec![-5, 0, 5]);
    }

    #[test]
    fn apply_delete_by_id_removes_only_matching_placements() {
        let mut s = ImageLayerState::new(1024);
        s.insert_placement(placement(1, 1, 0, 0, -1));
        s.insert_placement(placement(1, 2, 0, 0, -1));
        s.insert_placement(placement(2, 1, 0, 0, -1));
        s.apply_delete(&ImageDelete::ById(1));
        assert_eq!(s.placement_count(), 1);
    }

    #[test]
    fn apply_delete_by_placement_removes_one() {
        let mut s = ImageLayerState::new(1024);
        s.insert_placement(placement(1, 1, 0, 0, -1));
        s.insert_placement(placement(1, 2, 0, 0, -1));
        s.apply_delete(&ImageDelete::ByPlacement {
            image_id: 1,
            placement_id: 1,
        });
        assert_eq!(s.placement_count(), 1);
    }

    #[test]
    fn apply_delete_at_cursor_matches_row_col() {
        let mut s = ImageLayerState::new(1024);
        s.insert_placement(placement(1, 1, 3, 7, -1));
        s.insert_placement(placement(2, 1, 4, 7, -1));
        s.apply_delete(&ImageDelete::AtCursor { row: 3, col: 7 });
        assert_eq!(s.placement_count(), 1);
    }

    #[test]
    fn apply_delete_by_z_index_removes_layer() {
        let mut s = ImageLayerState::new(1024);
        s.insert_placement(placement(1, 1, 0, 0, -1));
        s.insert_placement(placement(2, 1, 0, 0, 0));
        s.apply_delete(&ImageDelete::ByZIndex(-1));
        assert_eq!(s.placement_count(), 1);
    }

    #[test]
    fn apply_delete_all_clears_placements_only() {
        let mut s = ImageLayerState::new(1024);
        s.record_image(1, 100);
        s.insert_placement(placement(1, 1, 0, 0, -1));
        s.apply_delete(&ImageDelete::All);
        assert_eq!(s.placement_count(), 0);
        // Image bytes stay (Kitty: 'a=d,d=a' removes display, not storage).
        assert_eq!(s.memory_used(), 100);
    }

    #[test]
    fn recompute_pixel_dims_clamps_to_one() {
        let mut s = ImageLayerState::new(1024);
        s.recompute_pixel_dims(0, 0);
        assert_eq!(s.cell_w(), 1);
        assert_eq!(s.cell_h(), 1);
        s.recompute_pixel_dims(9, 18);
        assert_eq!(s.cell_w(), 9);
        assert_eq!(s.cell_h(), 18);
    }

    #[test]
    fn resolve_pixel_placements_uses_cell_metrics_for_position() {
        let mut s = ImageLayerState::new(1024);
        s.recompute_pixel_dims(10, 20);
        s.insert_placement(placement(1, 1, 3, 5, -1));
        let resolved = s.resolve_pixel_placements(|_| Some((100, 100)));
        assert_eq!(resolved.len(), 1);
        let p = &resolved[0];
        assert_eq!(p.pixel_x, 50); // col 5 × cell_w 10
        assert_eq!(p.pixel_y, 60); // row 3 × cell_h 20
        assert_eq!(p.pixel_w, 100);
        assert_eq!(p.pixel_h, 100);
    }

    #[test]
    fn resolve_pixel_placements_respects_explicit_cols_rows() {
        let mut s = ImageLayerState::new(1024);
        s.recompute_pixel_dims(10, 20);
        let mut p = placement(1, 1, 0, 0, -1);
        p.columns = 4;
        p.rows = 2;
        s.insert_placement(p);
        let resolved = s.resolve_pixel_placements(|_| Some((999, 999)));
        // Explicit cols/rows win over the image's natural size.
        assert_eq!(resolved[0].pixel_w, 40); // 4 × 10
        assert_eq!(resolved[0].pixel_h, 40); // 2 × 20
    }

    #[test]
    fn resolve_pixel_placements_resize_anchor_stable() {
        // The same (row, col) anchor produces different pixel coords
        // after `recompute_pixel_dims` — that's the whole point.
        let mut s = ImageLayerState::new(1024);
        s.recompute_pixel_dims(10, 20);
        s.insert_placement(placement(1, 1, 2, 3, -1));
        let r1 = s.resolve_pixel_placements(|_| Some((80, 40)));
        s.recompute_pixel_dims(20, 40);
        let r2 = s.resolve_pixel_placements(|_| Some((80, 40)));
        // First frame: col 3 × 10 = 30; second: col 3 × 20 = 60.
        assert_eq!(r1[0].pixel_x, 30);
        assert_eq!(r2[0].pixel_x, 60);
    }

    // ── Event splitter / ingest ────────────────────────

    #[test]
    fn split_image_events_separates_responses() {
        let events = vec![
            ImageEvent::Response {
                data: "\x1b_Gi=1;OK\x1b\\".to_string(),
            },
            ImageEvent::Delete {
                target: ImageDelete::All,
            },
        ];
        let (state, resp) = split_image_events(events);
        assert_eq!(state.len(), 1);
        assert_eq!(resp.len(), 1);
        assert!(resp[0].contains("OK"));
    }

    #[test]
    fn ingest_state_events_records_image_and_evicts() {
        let mut s = ImageLayerState::new(300);
        let mut store: HashMap<u32, DecodedImage> = HashMap::new();
        let events = vec![
            ImageEvent::ImageReady {
                image: decoded(1, 100),
            },
            ImageEvent::ImageReady {
                image: decoded(2, 100),
            },
            ImageEvent::ImageReady {
                image: decoded(3, 100),
            },
            ImageEvent::ImageReady {
                image: decoded(4, 100),
            },
        ];
        let evicted = ingest_state_events(&mut s, events, &mut store);
        assert_eq!(evicted, vec![1]);
        // The DecodedImage store mirrors the state's eviction.
        assert!(!store.contains_key(&1));
        assert!(store.contains_key(&4));
    }

    #[test]
    fn ingest_state_events_place_inserts_placement_and_touches_lru() {
        let mut s = ImageLayerState::new(300);
        let mut store: HashMap<u32, DecodedImage> = HashMap::new();
        let _ = ingest_state_events(
            &mut s,
            vec![
                ImageEvent::ImageReady {
                    image: decoded(1, 100),
                },
                ImageEvent::ImageReady {
                    image: decoded(2, 100),
                },
                ImageEvent::ImageReady {
                    image: decoded(3, 100),
                },
            ],
            &mut store,
        );
        // Touch image 1 via Place.
        let _ = ingest_state_events(
            &mut s,
            vec![ImageEvent::Place {
                placement: placement(1, 1, 0, 0, -1),
            }],
            &mut store,
        );
        assert_eq!(s.placement_count(), 1);
        // Now insert image 4 → 2 should evict (1 was just touched).
        let evicted = ingest_state_events(
            &mut s,
            vec![ImageEvent::ImageReady {
                image: decoded(4, 100),
            }],
            &mut store,
        );
        assert_eq!(evicted, vec![2]);
    }
}

// ─────────────────────────────────────────────────────────────────────
// GPU-coupled façade.
//
// Kept thin on purpose: most of the logic lives in `ImageLayerState`. This
// type adds the wgpu texture map, the `ingest` entry point that wires the
// state machine to GPU uploads, and the lazy texture eviction so dropped
// images also drop their wgpu handle.
// ─────────────────────────────────────────────────────────────────────

/// A wgpu texture + view pair for one decoded image.
pub struct ImageTexture {
    #[allow(dead_code)] // Phase 5 keeps the texture alive for the bind group.
    pub texture: wgpu::Texture,
    pub view: wgpu::TextureView,
    pub width: u32,
    pub height: u32,
}

/// GPU-aware façade over [`ImageLayerState`]. Owns the per-image
/// `wgpu::Texture` handles and forwards everything else to the state
/// machine.
pub struct ImageLayer {
    pub state: ImageLayerState,
    pub textures: HashMap<u32, ImageTexture>,
    pub decoded: HashMap<u32, DecodedImage>,
}

impl ImageLayer {
    pub fn new(quota_bytes: u64) -> Self {
        Self {
            state: ImageLayerState::new(quota_bytes),
            textures: HashMap::new(),
            decoded: HashMap::new(),
        }
    }

    /// Forward [`ImageLayerState::recompute_pixel_dims`].
    pub fn recompute_pixel_dims(&mut self, cell_w: u32, cell_h: u32) {
        self.state.recompute_pixel_dims(cell_w, cell_h);
    }

    /// Ingest a batch of events (Response variants must already be split
    /// off via [`split_image_events`]). For every `ImageReady` the
    /// decoded RGBA is uploaded to a fresh wgpu texture; for every
    /// `Place`/`Delete` the state machine is updated. Evicted image
    /// textures are dropped immediately so wgpu releases their VRAM.
    pub fn ingest(&mut self, events: Vec<ImageEvent>, device: &wgpu::Device, queue: &wgpu::Queue) {
        // Pre-upload textures so the state machine can evict in one pass.
        for evt in &events {
            if let ImageEvent::ImageReady { image } = evt {
                let tex = upload_rgba_texture(device, queue, image);
                self.textures.insert(image.id, tex);
            }
        }
        let evicted = ingest_state_events(&mut self.state, events, &mut self.decoded);
        for victim in evicted {
            self.textures.remove(&victim);
        }
    }

    /// Resolve placements into pixel rectangles using the cached texture
    /// dimensions when the placement has no explicit `columns`/`rows`.
    pub fn resolve_placements(&self) -> Vec<PixelPlacement> {
        self.state
            .resolve_pixel_placements(|id| self.textures.get(&id).map(|t| (t.width, t.height)))
    }
}

/// Upload an un-premultiplied RGBA8 image to a fresh wgpu texture with
/// the `Rgba8UnormSrgb` view format (resolved OQ5 in the implementation
/// plan).
fn upload_rgba_texture(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    image: &DecodedImage,
) -> ImageTexture {
    let size = wgpu::Extent3d {
        width: image.width.max(1),
        height: image.height.max(1),
        depth_or_array_layers: 1,
    };
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("native-poc-image-texture"),
        size,
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8UnormSrgb,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    if !image.rgba_data.is_empty() {
        queue.write_texture(
            wgpu::ImageCopyTexture {
                texture: &texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &image.rgba_data,
            wgpu::ImageDataLayout {
                offset: 0,
                bytes_per_row: Some(4 * size.width),
                rows_per_image: Some(size.height),
            },
            size,
        );
    }
    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
    ImageTexture {
        texture,
        view,
        width: size.width,
        height: size.height,
    }
}
