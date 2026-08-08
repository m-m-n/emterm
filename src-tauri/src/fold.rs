//! Command-output folding: foldable regions and display↔actual line mapping.
//!
//! Port of the WebView build's `src/terminal/fold-manager.ts`
//! (`FoldManager`). A fold region is a contiguous run of absolute buffer
//! rows produced either by an OSC 133 C→D command zone or a custom
//! `OSC 777;emterm;fold` marker. When a region is *collapsed* its body is
//! hidden behind a single summary row, so the renderer paints fewer rows
//! than the buffer holds. [`FoldManager`] owns the registry of regions and
//! answers the line-mapping queries the renderer needs:
//! [`FoldManager::display_line_to_actual`] /
//! [`FoldManager::actual_line_to_display`] translate between the rows the
//! user sees and the rows in the buffer.
//!
//! "Absolute line" numbering matches the scroll model used elsewhere in the
//! native build (see [`crate::prompts`]): `0..scrollback_len` are scrollback
//! rows (0 = oldest), `scrollback_len + r` is viewport row `r`. This mirrors
//! the WebView `lineIndex` (`scrollbackLength + cursor.row`). When scrollback
//! evicts its oldest rows the whole frame shifts down, so
//! [`FoldManager::prune_before_line`] drops out-of-range regions and re-bases
//! the survivors — mirroring the WebView `pruneBeforeLine`.
//!
//! Region IDs are derived from `start_line` (`osc133:<start>` /
//! `custom:<start>`), exactly as the WebView builds them, so a re-base in
//! `prune_before_line` re-keys each survivor to its new start line.
//!
//! This module is **forward-staged**: the public API is exercised only by
//! the unit tests below today. The renderer / OSC-registration / click
//! wiring that consumes it lands in a later sub-phase, so we
//! `allow(dead_code)` at the module root (mirroring [`crate::mux`]) rather
//! than scattering attributes on each item — the intent stays in one place.
#![allow(dead_code)]

use std::collections::BTreeMap;

/// Where a [`FoldRegion`] came from. Determines the ID prefix and which of
/// `command_text` / `label` carries the summary text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FoldSource {
    /// An OSC 133 C→D command zone. Carries `command_text` + optional
    /// `exit_code`.
    Osc133,
    /// A custom `OSC 777;emterm;fold` marker. Carries `label`.
    Custom,
}

/// A foldable run of absolute buffer rows.
///
/// Mirrors the TypeScript `FoldRegion` interface. `start_line` is inclusive,
/// `end_line` is exclusive, so `line_count == end_line - start_line`. The
/// `command_text` / `label` split follows `source`: an `Osc133` region uses
/// `command_text` (and may carry an `exit_code`), a `Custom` region uses
/// `label`. The field not used by a given source is `None` (matching the
/// WebView's `undefined`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FoldRegion {
    /// Unique identifier, derived from `start_line`:
    /// `osc133:<start_line>` or `custom:<start_line>`.
    pub id: String,
    /// Absolute line index of the fold start (inclusive).
    pub start_line: u32,
    /// Absolute line index of the fold end (exclusive).
    pub end_line: u32,
    /// Whether the region is currently collapsed (body hidden).
    pub collapsed: bool,
    /// Source of this region.
    pub source: FoldSource,
    /// Command line text (set for `Osc133`, `None` for `Custom`).
    pub command_text: Option<String>,
    /// Fold label (set for `Custom`, `None` for `Osc133`).
    pub label: Option<String>,
    /// Exit code (set for `Osc133` when the `D` mark carried one).
    pub exit_code: Option<i32>,
    /// Number of buffer rows in the region (`end_line - start_line`).
    pub line_count: u32,
}

impl FoldRegion {
    /// Build the WebView-compatible region ID for a `(source, start_line)`
    /// pair. Centralized so registration and `prune_before_line`'s re-base
    /// agree on the format.
    fn make_id(source: FoldSource, start_line: u32) -> String {
        match source {
            FoldSource::Osc133 => format!("osc133:{start_line}"),
            FoldSource::Custom => format!("custom:{start_line}"),
        }
    }
}

/// Registry of fold regions plus the display↔actual line mapping.
///
/// Port of `FoldManager`. Regions are keyed by ID in a `BTreeMap` (the
/// WebView uses a `Map`; a `BTreeMap` gives deterministic iteration without
/// changing results, since regions never overlap). A cache of the collapsed
/// regions sorted by `start_line` is rebuilt lazily and invalidated whenever
/// the registry or any `collapsed` flag changes — mirroring the WebView's
/// `collapsedCache`.
#[derive(Debug, Default)]
pub struct FoldManager {
    /// All registered regions, keyed by [`FoldRegion::id`].
    regions: BTreeMap<String, FoldRegion>,
    /// Whether folding is enabled. Disabling unfolds all regions but keeps
    /// the region records (see [`Self::set_enabled`]).
    enabled: bool,
    /// Cached collapsed regions sorted by `start_line`; `None` when stale.
    collapsed_cache: Option<Vec<FoldRegion>>,
}

impl FoldManager {
    /// A fresh manager with folding enabled and no regions. Matches the
    /// WebView constructor (`enabled = true`).
    pub fn new() -> Self {
        FoldManager {
            regions: BTreeMap::new(),
            enabled: true,
            collapsed_cache: None,
        }
    }

    /// Register a foldable region from an OSC 133 C→D pair. A zero-or-
    /// negative line count is rejected (`end_line <= start_line`), as is a
    /// region that overlaps an existing one — mirroring `registerOsc133Region`.
    pub fn register_osc133_region(
        &mut self,
        start_line: u32,
        end_line: u32,
        command_text: String,
        exit_code: Option<i32>,
    ) {
        if end_line <= start_line {
            return;
        }
        if self.has_overlap(start_line, end_line) {
            return;
        }
        let id = FoldRegion::make_id(FoldSource::Osc133, start_line);
        let region = FoldRegion {
            id: id.clone(),
            start_line,
            end_line,
            collapsed: false,
            source: FoldSource::Osc133,
            command_text: Some(command_text),
            label: None,
            exit_code,
            line_count: end_line - start_line,
        };
        self.regions.insert(id, region);
        self.invalidate_cache();
    }

    /// Register a foldable region from a custom OSC fold marker. Same
    /// rejection rules as [`Self::register_osc133_region`]; an empty label
    /// falls back to `"..."` — mirroring `registerCustomRegion`.
    pub fn register_custom_region(&mut self, start_line: u32, end_line: u32, label: String) {
        if end_line <= start_line {
            return;
        }
        if self.has_overlap(start_line, end_line) {
            return;
        }
        let id = FoldRegion::make_id(FoldSource::Custom, start_line);
        let label = if label.is_empty() {
            "...".to_string()
        } else {
            label
        };
        let region = FoldRegion {
            id: id.clone(),
            start_line,
            end_line,
            collapsed: false,
            source: FoldSource::Custom,
            command_text: None,
            label: Some(label),
            exit_code: None,
            line_count: end_line - start_line,
        };
        self.regions.insert(id, region);
        self.invalidate_cache();
    }

    /// Toggle the collapsed state of the region containing `line_index`.
    /// Returns `true` when a region was toggled. A no-op (returns `false`)
    /// when folding is disabled or no region contains the line — mirroring
    /// `toggleFold`.
    pub fn toggle_fold(&mut self, line_index: u32) -> bool {
        if !self.enabled {
            return false;
        }
        let Some(id) = self.find_region_id_containing(line_index) else {
            return false;
        };
        let region = self.regions.get_mut(&id).expect("id from find");
        region.collapsed = !region.collapsed;
        self.invalidate_cache();
        true
    }

    /// The region containing `line_index` (if any). Mirrors `getRegionAtLine`.
    pub fn get_region_at_line(&self, line_index: u32) -> Option<&FoldRegion> {
        self.find_region_containing(line_index)
    }

    /// All collapsed regions sorted by `start_line`. The result is cached and
    /// reused until the registry or a `collapsed` flag changes. Mirrors
    /// `getCollapsedRegions`.
    ///
    /// The cache is built on first call after an invalidation; subsequent
    /// calls return a slice of the cached `Vec`.
    pub fn get_collapsed_regions(&mut self) -> &[FoldRegion] {
        if self.collapsed_cache.is_none() {
            let mut collapsed: Vec<FoldRegion> = self
                .regions
                .values()
                .filter(|r| r.collapsed)
                .cloned()
                .collect();
            collapsed.sort_by_key(|r| r.start_line);
            self.collapsed_cache = Some(collapsed);
        }
        self.collapsed_cache.as_ref().expect("just built")
    }

    /// Whether any region is currently collapsed. The renderer gates the
    /// fold-aware draw path on this (not on `enabled`) — mirroring the
    /// WebView's `getCollapsedRegions().length > 0` check. Computed without
    /// allocating the sorted cache.
    pub fn has_collapsed_regions(&self) -> bool {
        self.regions.values().any(|r| r.collapsed)
    }

    /// Map a display line index to its actual buffer line index.
    ///
    /// With no collapsed regions this is the identity. Otherwise each
    /// collapsed region above the line contributes a single summary row in
    /// the display while hiding `line_count - 1` body rows; a display line at
    /// a region's start *is* that summary row and maps straight to the start.
    /// Mirrors `displayLineToActual`.
    pub fn display_line_to_actual(&mut self, display_line: u32) -> u32 {
        let collapsed = self.get_collapsed_regions();
        if collapsed.is_empty() {
            return display_line;
        }
        let mut actual = display_line;
        for region in collapsed {
            if actual < region.start_line {
                break;
            }
            if actual == region.start_line {
                // This display line IS the summary line.
                return region.start_line;
            }
            // Display line is past this collapsed region's summary: add back
            // the hidden body rows (`line_count - 1`, since the summary
            // occupies one row).
            actual += region.line_count - 1;
        }
        actual
    }

    /// Map an actual buffer line index to its display line index.
    ///
    /// Lines inside a collapsed region collapse onto that region's summary
    /// row. Lines after a collapsed region shift up by the hidden body rows.
    /// Mirrors `actualLineToDisplay`.
    pub fn actual_line_to_display(&mut self, actual_line: u32) -> u32 {
        let collapsed = self.get_collapsed_regions();
        if collapsed.is_empty() {
            return actual_line;
        }
        let mut offset = 0u32;
        for region in collapsed {
            if actual_line < region.start_line {
                break;
            }
            if actual_line >= region.start_line && actual_line < region.end_line {
                // Inside a collapsed region: maps to the summary row.
                // Saturating guards against a stale region whose start_line
                // exceeds the accumulated offset (resize reflow regression).
                return region.start_line.saturating_sub(offset);
            }
            // Past this collapsed region: accumulate its hidden body rows.
            offset += region.line_count - 1;
        }
        // Saturating mirrors the WebView's Math.max clamping for stale regions.
        actual_line.saturating_sub(offset)
    }

    /// Whether `display_line` is the summary row of some collapsed region.
    /// Mirrors `isSummaryLine`.
    pub fn is_summary_line(&mut self, display_line: u32) -> bool {
        self.get_summary_region(display_line).is_some()
    }

    /// The region whose summary row is at `display_line`, if any. Mirrors
    /// `getSummaryRegion`. Returns an owned clone so the caller does not hold
    /// a borrow on the collapsed cache.
    pub fn get_summary_region(&mut self, display_line: u32) -> Option<FoldRegion> {
        let collapsed = self.get_collapsed_regions();
        if collapsed.is_empty() {
            return None;
        }
        let mut offset = 0u32;
        for region in collapsed {
            // Saturating mirrors the WebView's Math.max clamping for stale regions.
            let summary_display = region.start_line.saturating_sub(offset);
            if display_line == summary_display {
                return Some(region.clone());
            }
            if display_line < summary_display {
                break;
            }
            offset += region.line_count - 1;
        }
        None
    }

    /// Total display rows given `total_actual_lines` buffer rows: the buffer
    /// total minus the body rows hidden by every collapsed region. Mirrors
    /// `getTotalDisplayLines`. Saturating to match the WebView's `Math.max`
    /// clamping when a stale collapsed region outlives a resize reflow.
    pub fn get_total_display_lines(&mut self, total_actual_lines: u32) -> u32 {
        let collapsed = self.get_collapsed_regions();
        let hidden: u32 = collapsed.iter().map(|r| r.line_count - 1).sum();
        total_actual_lines.saturating_sub(hidden)
    }

    /// Expand the collapsed region containing `actual_line`. Returns `true`
    /// when a collapsed region was expanded; `false` when the line is in no
    /// region or in an already-expanded one. Mirrors `expandRegionContaining`.
    pub fn expand_region_containing(&mut self, actual_line: u32) -> bool {
        let Some(id) = self.find_region_id_containing(actual_line) else {
            return false;
        };
        let region = self.regions.get_mut(&id).expect("id from find");
        if !region.collapsed {
            return false;
        }
        region.collapsed = false;
        self.invalidate_cache();
        true
    }

    /// Expand every region. Mirrors `unfoldAll`.
    pub fn unfold_all(&mut self) {
        for region in self.regions.values_mut() {
            region.collapsed = false;
        }
        self.invalidate_cache();
    }

    /// Prune regions for `line_index` discarded scrollback rows and re-base
    /// the survivors. A region entirely before the boundary
    /// (`end_line <= line_index`) is dropped; one that spans the boundary
    /// (`start_line < line_index`) is also dropped, since its head was
    /// evicted. Survivors are shifted down by `line_index` and re-keyed to
    /// their new start line. Mirrors `pruneBeforeLine`.
    pub fn prune_before_line(&mut self, line_index: u32) {
        let mut new_regions: BTreeMap<String, FoldRegion> = BTreeMap::new();
        for region in self.regions.values() {
            if region.end_line <= line_index {
                // Entirely before the boundary: drop.
                continue;
            }
            if region.start_line < line_index {
                // Spans the boundary (partial overlap): drop.
                continue;
            }
            // After the boundary: shift down and re-key.
            let new_start = region.start_line - line_index;
            let new_end = region.end_line - line_index;
            let new_id = FoldRegion::make_id(region.source, new_start);
            new_regions.insert(
                new_id.clone(),
                FoldRegion {
                    id: new_id,
                    start_line: new_start,
                    end_line: new_end,
                    ..region.clone()
                },
            );
        }
        self.regions = new_regions;
        self.invalidate_cache();
    }

    /// Enable or disable folding. Disabling unfolds all regions (but keeps
    /// the region records, so re-enabling does not lose them). Mirrors
    /// `setEnabled`.
    pub fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
        if !enabled {
            self.unfold_all();
        }
    }

    /// Whether folding is enabled. Mirrors `isEnabled`.
    pub fn is_enabled(&self) -> bool {
        self.enabled
    }

    /// Test-only: number of registered fold regions. Used by the mux
    /// off-thread replay parity test to assert the off-thread swap registers
    /// the same OSC 133 C→D regions as the synchronous path.
    #[cfg(test)]
    pub(crate) fn region_count(&self) -> usize {
        self.regions.len()
    }

    // ── Private helpers ──────────────────────────────────────

    /// The region containing `line_index` (`start <= line < end`), if any.
    fn find_region_containing(&self, line_index: u32) -> Option<&FoldRegion> {
        self.regions
            .values()
            .find(|r| line_index >= r.start_line && line_index < r.end_line)
    }

    /// The ID of the region containing `line_index`, if any. A separate
    /// lookup so a caller can then take a `&mut` to the region without
    /// holding an immutable borrow across the mutation.
    fn find_region_id_containing(&self, line_index: u32) -> Option<String> {
        self.find_region_containing(line_index)
            .map(|r| r.id.clone())
    }

    /// Whether `[start_line, end_line)` overlaps any existing region. Uses
    /// the half-open overlap test `start < r.end && end > r.start`, matching
    /// the WebView `hasOverlap`.
    fn has_overlap(&self, start_line: u32, end_line: u32) -> bool {
        self.regions
            .values()
            .any(|r| start_line < r.end_line && end_line > r.start_line)
    }

    /// Drop the sorted collapsed-region cache so it is rebuilt on next read.
    fn invalidate_cache(&mut self) {
        self.collapsed_cache = None;
    }

    /// Build a per-frame [`FoldLayout`]: the display↔actual mapping for the
    /// visible window, captured into a value the renderer can query without
    /// holding a `&mut` borrow on the manager.
    ///
    /// `scrollback_len` is the number of scrollback rows (absolute rows
    /// `0..scrollback_len`); `viewport_rows` is the live viewport height;
    /// `scroll_offset` is rows back from live (`0` = live tail). This mirrors
    /// the WebView build's `getVisibleLinesWithFolding` /
    /// `renderFoldSummaryLines` setup (display_start computed against
    /// `getTotalDisplayLines`).
    ///
    /// The collapsed-region snapshot is cloned into the layout so the
    /// returned value answers [`FoldLayout::actual_line_to_display`] /
    /// [`FoldLayout::region_at_line`] for the search-highlight pass without
    /// re-borrowing the manager (the renderer reads `App` immutably while
    /// painting).
    ///
    /// The row list is built with a **single forward cursor** over the sorted
    /// collapsed regions (O(viewport_rows + collapsed_count)) rather than
    /// calling `get_summary_region` / `display_line_to_actual` per row, each
    /// of which scanned the collapsed list linearly (O(viewport × collapsed)).
    pub fn build_layout(
        &mut self,
        scrollback_len: u32,
        viewport_rows: u16,
        scroll_offset: u32,
    ) -> FoldLayout {
        let collapsed: Vec<FoldRegion> = self.get_collapsed_regions().to_vec();
        let total_actual = scrollback_len + viewport_rows as u32;
        let total_display = self.get_total_display_lines(total_actual);
        // display_start = max(0, total_display - viewport_rows - scroll_offset)
        // computed with saturating arithmetic to stay in `u32`.
        let display_start = total_display
            .saturating_sub(viewport_rows as u32)
            .saturating_sub(scroll_offset);

        // Build a prefix-sum of hidden body rows: hidden_prefix[i] is the
        // total hidden rows from collapsed[0..i].  Stored in FoldLayout so
        // actual_line_to_display can use binary search + O(1) offset lookup.
        let mut hidden_prefix: Vec<u32> = Vec::with_capacity(collapsed.len());
        let mut running = 0u32;
        for r in &collapsed {
            hidden_prefix.push(running);
            running += r.line_count - 1;
        }

        // ── Forward-cursor row walk (O(rows + collapsed)) ──────────────────
        //
        // `ci`  = index into `collapsed` of the next region we haven't yet
        //         passed; starts at the first region whose summary could be
        //         at or after `display_start`.
        // `offset` = total hidden body rows from all collapsed regions before
        //            the current cursor position (same invariant as in the
        //            linear mapping functions).
        //
        // Because display lines increase monotonically (0, 1, 2, …) we only
        // ever advance `ci` forward, yielding O(rows + collapsed) total work.

        // Seed the cursor at the first collapsed region whose summary line
        // (= start_line - accumulated_hidden_before_it) is ≤ display_start.
        // We advance by restoring accumulated offset for each region before
        // the window.
        let mut ci = 0usize;
        let mut offset = 0u32; // hidden rows accumulated before ci

        // Skip regions whose summary line is strictly before display_start.
        while ci < collapsed.len() {
            let summary_display = collapsed[ci].start_line.saturating_sub(offset);
            if summary_display >= display_start {
                break;
            }
            // This region's summary is above the window.  Check whether the
            // window starts inside the region's body (between summary+1 and
            // the last hidden row).  Either way we've consumed this region.
            offset += collapsed[ci].line_count - 1;
            ci += 1;
        }

        // Now walk display lines display_start .. display_start + viewport_rows,
        // advancing `ci` whenever we pass a collapsed region.
        let mut rows: Vec<FoldRowKind> = Vec::with_capacity(viewport_rows as usize);
        // `actual` tracks the actual buffer line corresponding to the current
        // display_line once we've applied `offset`.
        for r in 0..viewport_rows as u32 {
            let display_line = display_start + r;

            // Advance cursor past any region whose summary is strictly before
            // the current display_line (can happen if the window starts mid-gap).
            while ci < collapsed.len() {
                let summary_display = collapsed[ci].start_line.saturating_sub(offset);
                if summary_display > display_line {
                    break;
                }
                if summary_display == display_line {
                    // This display line IS the summary row for collapsed[ci].
                    break;
                }
                // summary_display < display_line: already passed it above the
                // window but the window start landed inside the body — skip.
                offset += collapsed[ci].line_count - 1;
                ci += 1;
            }

            if ci < collapsed.len() {
                let summary_display = collapsed[ci].start_line.saturating_sub(offset);
                if display_line == summary_display {
                    // Summary row: emit Summary and advance cursor.
                    rows.push(FoldRowKind::Summary {
                        region: collapsed[ci].clone(),
                    });
                    // Advance past this region (its body rows are all hidden).
                    offset += collapsed[ci].line_count - 1;
                    ci += 1;
                    continue;
                }
            }

            // Normal cell row: translate display → actual.
            let actual_line = display_line + offset;
            rows.push(FoldRowKind::Cells { actual_line });
        }

        FoldLayout {
            rows,
            display_start,
            collapsed,
            hidden_prefix,
        }
    }
}

/// What a single on-screen row resolves to once collapsed regions are
/// accounted for. Built by [`FoldManager::build_layout`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FoldRowKind {
    /// Paint the cells of buffer row `actual_line` at this screen row.
    Cells { actual_line: u32 },
    /// This screen row is a fold summary: skip the normal cell text and
    /// paint the region's summary overlay instead.
    Summary { region: FoldRegion },
}

/// A frozen, query-only view of the fold mapping for one rendered frame.
///
/// [`FoldManager::build_layout`] produces this while holding `&mut` on the
/// manager; the renderer then consults it immutably for cell row selection
/// ([`Self::rows`]), summary overlays, and search-highlight display-row
/// translation ([`Self::actual_line_to_display`] / [`Self::region_at_line`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FoldLayout {
    /// One entry per visible screen row (length == `viewport_rows`).
    pub rows: Vec<FoldRowKind>,
    /// Top display line of the visible window
    /// (`max(0, total_display - viewport_rows - scroll_offset)`). Search
    /// highlights subtract this from a match's display line to get its
    /// screen row.
    pub display_start: u32,
    /// Collapsed regions sorted by `start_line` (snapshot taken at build
    /// time). Backs the immutable line-mapping queries below.
    collapsed: Vec<FoldRegion>,
    /// Prefix sums of hidden body rows: `hidden_prefix[i]` is the total
    /// hidden rows from `collapsed[0..i]` (i.e., before region `i`).
    /// Length equals `collapsed.len()`. Used by `actual_line_to_display`
    /// to turn a binary-search result into an O(1) offset lookup.
    hidden_prefix: Vec<u32>,
}

impl FoldLayout {
    /// The collapsed region containing `actual_line`, if any. Immutable
    /// equivalent of [`FoldManager::get_region_at_line`] restricted to the
    /// collapsed set (search skips matches inside a collapsed region).
    ///
    /// Uses binary search on the sorted `start_line` values for O(log n)
    /// lookup instead of a linear scan.
    pub fn region_at_line(&self, actual_line: u32) -> Option<&FoldRegion> {
        // Binary search for the last region whose start_line ≤ actual_line.
        // partition_point gives the first index where start_line > actual_line,
        // so the candidate is at index - 1 (if index > 0).
        let idx = self
            .collapsed
            .partition_point(|r| r.start_line <= actual_line);
        if idx == 0 {
            return None;
        }
        let region = &self.collapsed[idx - 1];
        // Confirm actual_line is inside [start_line, end_line).
        if actual_line < region.end_line {
            Some(region)
        } else {
            None
        }
    }

    /// Map an actual buffer line to its display line using the frozen
    /// collapsed snapshot. Immutable equivalent of
    /// [`FoldManager::actual_line_to_display`].
    ///
    /// Uses binary search on `start_line` plus a O(1) prefix-sum lookup to
    /// compute the hidden-row offset in O(log n) rather than O(n).
    pub fn actual_line_to_display(&self, actual_line: u32) -> u32 {
        if self.collapsed.is_empty() {
            return actual_line;
        }
        // Find the last region whose start_line ≤ actual_line.
        let idx = self
            .collapsed
            .partition_point(|r| r.start_line <= actual_line);
        if idx == 0 {
            // actual_line is before every collapsed region: no offset.
            return actual_line;
        }
        let region = &self.collapsed[idx - 1];
        let offset_before = self.hidden_prefix[idx - 1];
        if actual_line < region.end_line {
            // Inside this collapsed region: maps to its summary row.
            // Saturating mirrors the WebView's Math.max clamping for stale regions.
            return region.start_line.saturating_sub(offset_before);
        }
        // Past this region: total offset = offset_before + this region's hidden rows.
        let total_offset = offset_before + (region.line_count - 1);
        // Saturating mirrors the WebView's Math.max clamping for stale regions.
        actual_line.saturating_sub(total_offset)
    }
}

#[cfg(test)]
mod tests;
