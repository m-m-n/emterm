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
mod tests {
    use super::*;

    // ── Registration ─────────────────────────────────────────

    #[test]
    fn register_osc133_sets_all_properties() {
        let mut fm = FoldManager::new();
        fm.register_osc133_region(5, 15, "ls -la".to_string(), Some(0));

        let r = fm.get_region_at_line(5).expect("region present");
        assert_eq!(r.source, FoldSource::Osc133);
        assert_eq!(r.start_line, 5);
        assert_eq!(r.end_line, 15);
        assert_eq!(r.command_text.as_deref(), Some("ls -la"));
        assert_eq!(r.exit_code, Some(0));
        assert_eq!(r.line_count, 10);
        assert!(!r.collapsed);
        assert_eq!(r.label, None);
        assert_eq!(r.id, "osc133:5");
    }

    #[test]
    fn register_custom_sets_label_no_exit_code() {
        let mut fm = FoldManager::new();
        fm.register_custom_region(10, 30, "Build Output".to_string());

        let r = fm.get_region_at_line(10).expect("region present");
        assert_eq!(r.source, FoldSource::Custom);
        assert_eq!(r.label.as_deref(), Some("Build Output"));
        assert_eq!(r.exit_code, None);
        assert_eq!(r.command_text, None);
        assert_eq!(r.line_count, 20);
        assert_eq!(r.id, "custom:10");
    }

    #[test]
    fn region_with_zero_lines_not_registered() {
        let mut fm = FoldManager::new();
        fm.register_osc133_region(5, 5, "echo hi".to_string(), Some(0));
        assert!(fm.get_region_at_line(5).is_none());
    }

    #[test]
    fn region_with_one_line_registered() {
        let mut fm = FoldManager::new();
        fm.register_osc133_region(5, 6, "echo hi".to_string(), Some(0));
        let r = fm.get_region_at_line(5).expect("region present");
        assert_eq!(r.line_count, 1);
    }

    #[test]
    fn osc133_without_exit_code() {
        let mut fm = FoldManager::new();
        fm.register_osc133_region(5, 15, "running...".to_string(), None);
        let r = fm.get_region_at_line(5).expect("region present");
        assert_eq!(r.exit_code, None);
    }

    #[test]
    fn custom_empty_label_falls_back() {
        let mut fm = FoldManager::new();
        fm.register_custom_region(10, 20, String::new());
        let r = fm.get_region_at_line(10).expect("region present");
        assert_eq!(r.label.as_deref(), Some("..."));
    }

    #[test]
    fn overlapping_region_does_not_overwrite() {
        let mut fm = FoldManager::new();
        fm.register_osc133_region(5, 15, "first".to_string(), Some(0));
        // 8..20 overlaps 5..15 → rejected.
        fm.register_osc133_region(8, 20, "second".to_string(), Some(1));
        let r = fm.get_region_at_line(5).expect("first still present");
        assert_eq!(r.command_text.as_deref(), Some("first"));
    }

    #[test]
    fn touching_regions_do_not_overlap() {
        // Half-open ranges: 5..10 and 10..15 share no row, so both register.
        let mut fm = FoldManager::new();
        fm.register_osc133_region(5, 10, "first".to_string(), Some(0));
        fm.register_osc133_region(10, 15, "second".to_string(), Some(0));
        assert_eq!(
            fm.get_region_at_line(5).unwrap().command_text.as_deref(),
            Some("first")
        );
        assert_eq!(
            fm.get_region_at_line(10).unwrap().command_text.as_deref(),
            Some("second")
        );
    }

    #[test]
    fn long_command_and_label_preserved() {
        let mut fm = FoldManager::new();
        let long_cmd = "a".repeat(200);
        fm.register_osc133_region(5, 15, long_cmd.clone(), Some(0));
        assert_eq!(
            fm.get_region_at_line(5).unwrap().command_text,
            Some(long_cmd)
        );

        let mut fm2 = FoldManager::new();
        let long_label = "b".repeat(200);
        fm2.register_custom_region(5, 15, long_label.clone());
        assert_eq!(fm2.get_region_at_line(5).unwrap().label, Some(long_label));
    }

    // ── Toggle ───────────────────────────────────────────────

    #[test]
    fn toggle_collapses_then_expands() {
        let mut fm = FoldManager::new();
        fm.register_osc133_region(5, 15, "test".to_string(), Some(0));

        assert!(fm.toggle_fold(5));
        assert!(fm.get_region_at_line(5).unwrap().collapsed);

        assert!(fm.toggle_fold(5));
        assert!(!fm.get_region_at_line(5).unwrap().collapsed);
    }

    #[test]
    fn toggle_on_missing_line_returns_false() {
        let mut fm = FoldManager::new();
        fm.register_osc133_region(5, 15, "test".to_string(), Some(0));
        assert!(!fm.toggle_fold(20));
    }

    #[test]
    fn toggle_on_interior_line_collapses_region() {
        let mut fm = FoldManager::new();
        fm.register_osc133_region(5, 15, "test".to_string(), Some(0));
        assert!(fm.toggle_fold(10));
        assert!(fm.get_region_at_line(5).unwrap().collapsed);
    }

    // ── get_region_at_line ───────────────────────────────────

    #[test]
    fn region_at_line_inside_returns_region() {
        let mut fm = FoldManager::new();
        fm.register_osc133_region(5, 15, "test".to_string(), Some(0));
        assert!(fm.get_region_at_line(5).is_some());
        assert!(fm.get_region_at_line(10).is_some());
        assert!(fm.get_region_at_line(14).is_some());
    }

    #[test]
    fn region_at_line_outside_returns_none() {
        let mut fm = FoldManager::new();
        fm.register_osc133_region(5, 15, "test".to_string(), Some(0));
        assert!(fm.get_region_at_line(4).is_none());
        assert!(fm.get_region_at_line(15).is_none());
        assert!(fm.get_region_at_line(100).is_none());
    }

    #[test]
    fn region_at_line_with_multiple_regions() {
        let mut fm = FoldManager::new();
        fm.register_osc133_region(5, 15, "first".to_string(), Some(0));
        fm.register_osc133_region(20, 30, "second".to_string(), Some(1));

        assert_eq!(
            fm.get_region_at_line(10).unwrap().command_text.as_deref(),
            Some("first")
        );
        assert_eq!(
            fm.get_region_at_line(25).unwrap().command_text.as_deref(),
            Some("second")
        );
        assert!(fm.get_region_at_line(17).is_none());
    }

    #[test]
    fn region_at_line_on_empty_returns_none() {
        let fm = FoldManager::new();
        assert!(fm.get_region_at_line(0).is_none());
        assert!(fm.get_region_at_line(100).is_none());
    }

    // ── get_collapsed_regions ────────────────────────────────

    #[test]
    fn collapsed_regions_only_collapsed_sorted() {
        let mut fm = FoldManager::new();
        fm.register_osc133_region(20, 30, "second".to_string(), Some(0));
        fm.register_osc133_region(5, 15, "first".to_string(), Some(0));
        fm.register_osc133_region(40, 50, "third".to_string(), Some(0));

        fm.toggle_fold(5);
        fm.toggle_fold(40);

        let collapsed = fm.get_collapsed_regions();
        assert_eq!(collapsed.len(), 2);
        assert_eq!(collapsed[0].start_line, 5);
        assert_eq!(collapsed[1].start_line, 40);
    }

    #[test]
    fn collapsed_cache_reflects_toggle_after_read() {
        // Reading builds the cache; a subsequent toggle must invalidate it so
        // the next read sees the new state.
        let mut fm = FoldManager::new();
        fm.register_osc133_region(5, 15, "test".to_string(), Some(0));
        assert_eq!(fm.get_collapsed_regions().len(), 0);
        fm.toggle_fold(5);
        assert_eq!(fm.get_collapsed_regions().len(), 1);
        fm.toggle_fold(5);
        assert_eq!(fm.get_collapsed_regions().len(), 0);
    }

    #[test]
    fn has_collapsed_regions_tracks_state() {
        let mut fm = FoldManager::new();
        fm.register_osc133_region(5, 15, "test".to_string(), Some(0));
        assert!(!fm.has_collapsed_regions());
        fm.toggle_fold(5);
        assert!(fm.has_collapsed_regions());
        fm.toggle_fold(5);
        assert!(!fm.has_collapsed_regions());
    }

    // ── Line mapping: display_line_to_actual ─────────────────

    #[test]
    fn display_to_actual_identity_when_no_collapse() {
        let mut fm = FoldManager::new();
        fm.register_osc133_region(5, 15, "test".to_string(), Some(0));
        // Registered but not collapsed → identity mapping.
        assert_eq!(fm.display_line_to_actual(0), 0);
        assert_eq!(fm.display_line_to_actual(10), 10);
        assert_eq!(fm.display_line_to_actual(20), 20);
    }

    #[test]
    fn display_to_actual_one_fold() {
        let mut fm = FoldManager::new();
        fm.register_osc133_region(5, 15, "test".to_string(), Some(0));
        fm.toggle_fold(5);

        // Before the fold: identity.
        assert_eq!(fm.display_line_to_actual(0), 0);
        assert_eq!(fm.display_line_to_actual(4), 4);
        // Summary row.
        assert_eq!(fm.display_line_to_actual(5), 5);
        // First row after the summary skips the 9 hidden rows.
        assert_eq!(fm.display_line_to_actual(6), 15);
        assert_eq!(fm.display_line_to_actual(7), 16);
    }

    #[test]
    fn display_to_actual_multiple_folds() {
        let mut fm = FoldManager::new();
        fm.register_osc133_region(5, 15, "first".to_string(), Some(0));
        fm.register_osc133_region(25, 35, "second".to_string(), Some(1));
        fm.toggle_fold(5);
        fm.toggle_fold(25);

        assert_eq!(fm.display_line_to_actual(4), 4);
        assert_eq!(fm.display_line_to_actual(5), 5);
        assert_eq!(fm.display_line_to_actual(6), 15);
        // Second fold's summary sits at display 16 (actual 25 - 9 hidden).
        assert_eq!(fm.display_line_to_actual(16), 25);
        // After both folds (18 hidden total).
        assert_eq!(fm.display_line_to_actual(17), 35);
    }

    #[test]
    fn display_to_actual_out_of_range_above_all_folds() {
        // A display line below every fold start stays identity (no fold
        // contributes an offset because `actual < start_line` breaks early).
        let mut fm = FoldManager::new();
        fm.register_osc133_region(50, 60, "test".to_string(), Some(0));
        fm.toggle_fold(50);
        assert_eq!(fm.display_line_to_actual(3), 3);
    }

    // ── Line mapping: actual_line_to_display ─────────────────

    #[test]
    fn actual_to_display_one_fold() {
        let mut fm = FoldManager::new();
        fm.register_osc133_region(5, 15, "test".to_string(), Some(0));
        fm.toggle_fold(5);

        assert_eq!(fm.actual_line_to_display(0), 0);
        assert_eq!(fm.actual_line_to_display(4), 4);
        // Start of the fold = summary row.
        assert_eq!(fm.actual_line_to_display(5), 5);
        // Interior of a collapsed region collapses onto the summary row.
        assert_eq!(fm.actual_line_to_display(10), 5);
        // After the fold: actual 15 → display 6.
        assert_eq!(fm.actual_line_to_display(15), 6);
        assert_eq!(fm.actual_line_to_display(16), 7);
    }

    #[test]
    fn round_trip_no_collapse_is_identity() {
        let mut fm = FoldManager::new();
        fm.register_osc133_region(5, 15, "test".to_string(), Some(0));
        for line in [0u32, 4, 5, 10, 14, 15, 20, 100] {
            let display = fm.actual_line_to_display(line);
            assert_eq!(fm.display_line_to_actual(display), line);
        }
    }

    #[test]
    fn round_trip_single_collapse_outside_body() {
        // For rows outside the collapsed body, display→actual→display and
        // actual→display→actual both round-trip cleanly.
        let mut fm = FoldManager::new();
        fm.register_osc133_region(5, 15, "test".to_string(), Some(0));
        fm.toggle_fold(5);
        // Actual rows outside [6, 15) (the hidden body) round-trip.
        for actual in [0u32, 4, 5, 15, 16, 30] {
            let display = fm.actual_line_to_display(actual);
            assert_eq!(
                fm.display_line_to_actual(display),
                actual,
                "actual {actual} did not round-trip (display {display})"
            );
        }
    }

    #[test]
    fn round_trip_multiple_collapse_outside_body() {
        let mut fm = FoldManager::new();
        fm.register_osc133_region(5, 15, "first".to_string(), Some(0));
        fm.register_osc133_region(25, 35, "second".to_string(), Some(1));
        fm.toggle_fold(5);
        fm.toggle_fold(25);
        // Rows outside both hidden bodies round-trip.
        for actual in [0u32, 4, 5, 15, 16, 24, 25, 35, 36, 100] {
            let display = fm.actual_line_to_display(actual);
            assert_eq!(
                fm.display_line_to_actual(display),
                actual,
                "actual {actual} did not round-trip (display {display})"
            );
        }
    }

    #[test]
    fn round_trip_adjacent_collapsed_regions() {
        // Two adjacent collapsed regions (5..10, 10..15). Each summary row
        // and each post-region row must round-trip.
        let mut fm = FoldManager::new();
        fm.register_osc133_region(5, 10, "first".to_string(), Some(0));
        fm.register_osc133_region(10, 15, "second".to_string(), Some(0));
        fm.toggle_fold(5);
        fm.toggle_fold(10);
        for actual in [0u32, 4, 5, 10, 15, 16, 50] {
            let display = fm.actual_line_to_display(actual);
            assert_eq!(
                fm.display_line_to_actual(display),
                actual,
                "actual {actual} did not round-trip (display {display})"
            );
        }
    }

    // ── Summary line queries ─────────────────────────────────

    #[test]
    fn is_summary_line_only_at_summary() {
        let mut fm = FoldManager::new();
        fm.register_osc133_region(5, 15, "test".to_string(), Some(0));
        fm.toggle_fold(5);
        assert!(fm.is_summary_line(5));
        assert!(!fm.is_summary_line(4));
        assert!(!fm.is_summary_line(6));
    }

    #[test]
    fn summary_region_returns_region_for_summary_line() {
        let mut fm = FoldManager::new();
        fm.register_osc133_region(5, 15, "test cmd".to_string(), Some(0));
        fm.toggle_fold(5);

        let r = fm.get_summary_region(5).expect("summary region present");
        assert_eq!(r.command_text.as_deref(), Some("test cmd"));
        assert!(fm.get_summary_region(4).is_none());
        assert!(fm.get_summary_region(6).is_none());
    }

    #[test]
    fn summary_region_none_when_no_collapse() {
        let mut fm = FoldManager::new();
        fm.register_osc133_region(5, 15, "test".to_string(), Some(0));
        // Not collapsed → no summary rows at all.
        assert!(fm.get_summary_region(5).is_none());
    }

    // ── get_total_display_lines ──────────────────────────────

    #[test]
    fn total_display_lines() {
        let mut fm = FoldManager::new();
        fm.register_osc133_region(5, 15, "test".to_string(), Some(0));
        // No collapse: total unchanged.
        assert_eq!(fm.get_total_display_lines(100), 100);
        // Collapse hides 9 body rows.
        fm.toggle_fold(5);
        assert_eq!(fm.get_total_display_lines(100), 91);
    }

    // ── Pruning ──────────────────────────────────────────────

    #[test]
    fn prune_removes_old_and_rebases() {
        let mut fm = FoldManager::new();
        fm.register_osc133_region(5, 15, "old".to_string(), Some(0));
        fm.register_osc133_region(25, 35, "new".to_string(), Some(0));

        fm.prune_before_line(20);

        // Old region (5..15, before boundary 20) is gone; new region 25..35
        // re-bases to 5..15 and is re-keyed.
        let r = fm.get_region_at_line(5).expect("rebased region present");
        assert_eq!(r.command_text.as_deref(), Some("new"));
        assert_eq!(r.start_line, 5);
        assert_eq!(r.end_line, 15);
        assert_eq!(r.id, "osc133:5");
        assert!(fm.get_region_at_line(0).is_none());
    }

    #[test]
    fn prune_adjusts_remaining_indices() {
        let mut fm = FoldManager::new();
        fm.register_osc133_region(20, 30, "test".to_string(), Some(0));
        fm.prune_before_line(10);

        let r = fm.get_region_at_line(10).expect("region present");
        assert_eq!(r.start_line, 10);
        assert_eq!(r.end_line, 20);
        assert_eq!(r.id, "osc133:10");
    }

    #[test]
    fn prune_removes_region_spanning_boundary() {
        let mut fm = FoldManager::new();
        fm.register_osc133_region(5, 15, "spanning".to_string(), Some(0));
        fm.register_osc133_region(20, 30, "after".to_string(), Some(0));

        fm.prune_before_line(10);
        // 5..15 spans boundary 10 → removed; 20..30 → 10..20.
        assert_eq!(fm.get_collapsed_regions().len(), 0);
        let r = fm.get_region_at_line(10).expect("after region present");
        assert_eq!(r.command_text.as_deref(), Some("after"));
    }

    #[test]
    fn prune_preserves_collapsed_state() {
        let mut fm = FoldManager::new();
        fm.register_osc133_region(20, 30, "test".to_string(), Some(0));
        fm.toggle_fold(20);

        fm.prune_before_line(10);
        let r = fm.get_region_at_line(10).expect("region present");
        assert!(r.collapsed);
    }

    #[test]
    fn prune_rebases_custom_id() {
        // A custom region's re-keyed ID keeps the `custom:` prefix.
        let mut fm = FoldManager::new();
        fm.register_custom_region(20, 30, "label".to_string());
        fm.prune_before_line(10);
        let r = fm.get_region_at_line(10).expect("region present");
        assert_eq!(r.id, "custom:10");
        assert_eq!(r.label.as_deref(), Some("label"));
    }

    #[test]
    fn prune_on_empty_does_not_panic() {
        let mut fm = FoldManager::new();
        fm.prune_before_line(10);
        assert!(fm.get_region_at_line(0).is_none());
    }

    // ── unfold_all ───────────────────────────────────────────

    #[test]
    fn unfold_all_expands_everything() {
        let mut fm = FoldManager::new();
        fm.register_osc133_region(5, 15, "first".to_string(), Some(0));
        fm.register_osc133_region(20, 30, "second".to_string(), Some(0));
        fm.toggle_fold(5);
        fm.toggle_fold(20);
        assert_eq!(fm.get_collapsed_regions().len(), 2);

        fm.unfold_all();
        assert_eq!(fm.get_collapsed_regions().len(), 0);
        assert!(!fm.get_region_at_line(5).unwrap().collapsed);
        assert!(!fm.get_region_at_line(20).unwrap().collapsed);
    }

    #[test]
    fn unfold_all_on_empty_does_not_panic() {
        let mut fm = FoldManager::new();
        fm.unfold_all();
        assert_eq!(fm.get_collapsed_regions().len(), 0);
    }

    // ── Enabled / disabled ───────────────────────────────────

    #[test]
    fn disabled_prevents_toggle() {
        let mut fm = FoldManager::new();
        fm.register_osc133_region(5, 15, "test".to_string(), Some(0));
        fm.set_enabled(false);
        assert!(!fm.toggle_fold(5));
        assert!(!fm.get_region_at_line(5).unwrap().collapsed);
    }

    #[test]
    fn set_enabled_false_unfolds_all() {
        let mut fm = FoldManager::new();
        fm.register_osc133_region(5, 15, "test".to_string(), Some(0));
        fm.toggle_fold(5);
        assert_eq!(fm.get_collapsed_regions().len(), 1);

        fm.set_enabled(false);
        assert_eq!(fm.get_collapsed_regions().len(), 0);
        // The region record itself survives the disable.
        assert!(fm.get_region_at_line(5).is_some());
    }

    #[test]
    fn set_enabled_true_after_disabled_allows_toggle() {
        let mut fm = FoldManager::new();
        fm.register_osc133_region(5, 15, "test".to_string(), Some(0));
        fm.set_enabled(false);
        fm.set_enabled(true);
        assert!(fm.toggle_fold(5));
        assert!(fm.get_region_at_line(5).unwrap().collapsed);
    }

    #[test]
    fn is_enabled_reflects_state() {
        let mut fm = FoldManager::new();
        assert!(fm.is_enabled());
        fm.set_enabled(false);
        assert!(!fm.is_enabled());
        fm.set_enabled(true);
        assert!(fm.is_enabled());
    }

    // ── expand_region_containing ─────────────────────────────

    #[test]
    fn expand_region_containing_expands_collapsed() {
        let mut fm = FoldManager::new();
        fm.register_osc133_region(5, 15, "test".to_string(), Some(0));
        fm.toggle_fold(5);
        assert!(fm.get_region_at_line(5).unwrap().collapsed);

        assert!(fm.expand_region_containing(10));
        assert!(!fm.get_region_at_line(5).unwrap().collapsed);
    }

    #[test]
    fn expand_region_containing_false_when_not_collapsed_or_outside() {
        let mut fm = FoldManager::new();
        fm.register_osc133_region(5, 15, "test".to_string(), Some(0));
        // In an expanded region → false.
        assert!(!fm.expand_region_containing(10));
        // Outside any region → false.
        assert!(!fm.expand_region_containing(20));
    }

    // ── Edge cases ───────────────────────────────────────────

    #[test]
    fn adjacent_regions_are_independent() {
        let mut fm = FoldManager::new();
        fm.register_osc133_region(5, 10, "first".to_string(), Some(0));
        fm.register_osc133_region(10, 15, "second".to_string(), Some(0));

        fm.toggle_fold(5);
        assert!(fm.get_region_at_line(5).unwrap().collapsed);
        assert!(!fm.get_region_at_line(10).unwrap().collapsed);
    }

    // ── Forward-cursor build_layout boundaries ───────────────

    #[test]
    fn build_layout_cursor_region_just_before_window() {
        // Region 2..5 is collapsed and its summary (display line 2) is
        // entirely above display_start = 5. The forward cursor must skip it
        // so that rows inside the window get the correct actual lines.
        //
        // scrollback_len=10, viewport=5, offset=0.
        // total_actual=15, hidden=4 (line_count=5, hides 4), total_display=11.
        // display_start = 11 - 5 - 0 = 6.
        // display 6 → actual 6 + 4 = 10, display 7 → actual 11, ...
        let mut fm = FoldManager::new();
        fm.register_osc133_region(2, 7, "early".to_string(), Some(0)); // line_count=5, hides 4
        fm.toggle_fold(2);
        let layout = fm.build_layout(10, 5, 0);
        assert_eq!(layout.display_start, 6);
        assert_eq!(layout.rows.len(), 5);
        // Display 6..10 are all normal cells shifted by 4 hidden rows.
        for (r, kind) in layout.rows.iter().enumerate() {
            match kind {
                FoldRowKind::Cells { actual_line } => {
                    assert_eq!(
                        *actual_line,
                        6 + 4 + r as u32,
                        "row {r}: expected actual {}, got {actual_line}",
                        6 + 4 + r as u32
                    );
                }
                FoldRowKind::Summary { .. } => panic!("no summary expected at row {r}"),
            }
        }
    }

    #[test]
    fn build_layout_cursor_region_spans_window_start() {
        // Region 0..20 is collapsed.  Even with scroll_offset placing
        // display_start inside the gap after the summary, the layout must not
        // emit a second summary row mid-window.
        //
        // total_actual = 30+5=35, hidden=19, total_display=16.
        // With scroll_offset=0: display_start = 16-5-0 = 11.
        // The collapsed region's summary is at display 0. display_start (11)
        // is deep inside the post-region area.  Rows 11..15 map to actual
        // 11+19=30, 31, 32, 33, 34.
        let mut fm = FoldManager::new();
        fm.register_osc133_region(0, 20, "big".to_string(), Some(0)); // hides 19
        fm.toggle_fold(0);
        let layout = fm.build_layout(30, 5, 0);
        assert_eq!(layout.display_start, 11);
        for (r, kind) in layout.rows.iter().enumerate() {
            match kind {
                FoldRowKind::Cells { actual_line } => {
                    assert_eq!(
                        *actual_line,
                        30 + r as u32,
                        "row {r} expected actual {}",
                        30 + r as u32
                    );
                }
                FoldRowKind::Summary { .. } => panic!("unexpected summary at row {r}"),
            }
        }
    }

    #[test]
    fn build_layout_cursor_summary_at_window_start() {
        // Region 0..5 collapsed; display_start lands exactly on the summary.
        // scrollback_len=10, viewport=5, offset such that display_start=0.
        // total_actual=15, hidden=4, total_display=11, offset=11-5=6.
        let mut fm = FoldManager::new();
        fm.register_osc133_region(0, 5, "top".to_string(), Some(0));
        fm.toggle_fold(0);
        let layout = fm.build_layout(10, 5, 6);
        assert_eq!(layout.display_start, 0);
        // Row 0 must be the summary.
        match &layout.rows[0] {
            FoldRowKind::Summary { region } => assert_eq!(region.start_line, 0),
            other => panic!("expected summary, got {other:?}"),
        }
        // Rows 1..4 must be normal cells starting at actual 5.
        for (r, kind) in layout.rows[1..].iter().enumerate() {
            match kind {
                FoldRowKind::Cells { actual_line } => {
                    assert_eq!(*actual_line, 5 + r as u32);
                }
                FoldRowKind::Summary { .. } => panic!("unexpected summary at row {}", r + 1),
            }
        }
    }

    #[test]
    fn build_layout_cursor_two_consecutive_collapsed() {
        // Two adjacent collapsed regions 0..3 and 3..6 (each hides 2 rows).
        // With display_start=0, rows 0,1 are summaries; row 2 is actual 6.
        //
        // total_actual=10+4=14, hidden=4, total_display=10.
        // offset=10-4-0=6; but let's use large scroll_offset to pin display_start=0.
        let mut fm = FoldManager::new();
        fm.register_osc133_region(0, 3, "first".to_string(), Some(0)); // hides 2
        fm.register_osc133_region(3, 6, "second".to_string(), Some(0)); // hides 2
        fm.toggle_fold(0);
        fm.toggle_fold(3);
        // total_actual=10+4=14, hidden=4, total_display=10.
        // display_start = 10-4-9999 saturates to 0.
        let layout = fm.build_layout(10, 4, 9999);
        assert_eq!(layout.display_start, 0);
        // Row 0 = summary for 0..3.
        match &layout.rows[0] {
            FoldRowKind::Summary { region } => assert_eq!(region.start_line, 0),
            other => panic!("row 0: expected summary, got {other:?}"),
        }
        // Row 1 = summary for 3..6.
        match &layout.rows[1] {
            FoldRowKind::Summary { region } => assert_eq!(region.start_line, 3),
            other => panic!("row 1: expected summary, got {other:?}"),
        }
        // Row 2 = actual 6.
        assert_eq!(layout.rows[2], FoldRowKind::Cells { actual_line: 6 });
        // Row 3 = actual 7.
        assert_eq!(layout.rows[3], FoldRowKind::Cells { actual_line: 7 });
    }

    // ── FoldLayout binary-search boundaries ──────────────────

    #[test]
    fn layout_region_at_line_boundary_before_region() {
        // actual_line == start_line - 1 must return None.
        let mut fm = FoldManager::new();
        fm.register_osc133_region(10, 20, "r".to_string(), Some(0));
        fm.toggle_fold(10);
        let layout = fm.build_layout(30, 5, 0);
        assert!(layout.region_at_line(9).is_none());
    }

    #[test]
    fn layout_region_at_line_boundary_at_start() {
        let mut fm = FoldManager::new();
        fm.register_osc133_region(10, 20, "r".to_string(), Some(0));
        fm.toggle_fold(10);
        let layout = fm.build_layout(30, 5, 0);
        assert!(layout.region_at_line(10).is_some());
    }

    #[test]
    fn layout_region_at_line_boundary_at_end_exclusive() {
        let mut fm = FoldManager::new();
        fm.register_osc133_region(10, 20, "r".to_string(), Some(0));
        fm.toggle_fold(10);
        let layout = fm.build_layout(30, 5, 0);
        // end_line is exclusive.
        assert!(layout.region_at_line(20).is_none());
        assert!(layout.region_at_line(19).is_some());
    }

    #[test]
    fn layout_actual_to_display_bsearch_row_before_region() {
        // actual_line just before a collapsed region: no offset applied.
        let mut fm = FoldManager::new();
        fm.register_osc133_region(10, 20, "r".to_string(), Some(0)); // hides 9
        fm.toggle_fold(10);
        let layout = fm.build_layout(30, 5, 0);
        // actual 9 → display 9 (no collapsed region before it).
        assert_eq!(layout.actual_line_to_display(9), 9);
    }

    #[test]
    fn layout_actual_to_display_bsearch_summary_row() {
        // actual_line == start_line of collapsed region → summary row.
        let mut fm = FoldManager::new();
        fm.register_osc133_region(10, 20, "r".to_string(), Some(0));
        fm.toggle_fold(10);
        let layout = fm.build_layout(30, 5, 0);
        // No regions before this one: offset_before=0. Summary at display 10.
        assert_eq!(layout.actual_line_to_display(10), 10);
    }

    #[test]
    fn layout_actual_to_display_bsearch_after_region() {
        // actual_line just after a collapsed region: offset = line_count - 1.
        let mut fm = FoldManager::new();
        fm.register_osc133_region(10, 20, "r".to_string(), Some(0)); // hides 9
        fm.toggle_fold(10);
        let layout = fm.build_layout(30, 5, 0);
        // actual 20 → display 20 - 9 = 11.
        assert_eq!(layout.actual_line_to_display(20), 11);
    }

    #[test]
    fn layout_actual_to_display_bsearch_two_regions() {
        // Two collapsed regions; verify prefix-sum is applied correctly.
        let mut fm = FoldManager::new();
        fm.register_osc133_region(5, 15, "first".to_string(), Some(0)); // hides 9
        fm.register_osc133_region(25, 35, "second".to_string(), Some(1)); // hides 9
        fm.toggle_fold(5);
        fm.toggle_fold(25);
        let layout = fm.build_layout(40, 5, 0);
        // Before first region.
        assert_eq!(layout.actual_line_to_display(4), 4);
        // Summary of first.
        assert_eq!(layout.actual_line_to_display(5), 5);
        // After first, before second: offset 9.
        assert_eq!(layout.actual_line_to_display(15), 6);
        assert_eq!(layout.actual_line_to_display(24), 15);
        // Summary of second: start 25 - 9 = 16.
        assert_eq!(layout.actual_line_to_display(25), 16);
        // After both regions: offset 18.
        assert_eq!(layout.actual_line_to_display(35), 17);
        assert_eq!(layout.actual_line_to_display(100), 82);
    }

    #[test]
    fn layout_actual_to_display_matches_manager_bsearch() {
        // Confirm the binary-search implementation agrees with FoldManager
        // across a range of actual lines, including region boundaries.
        let mut fm = FoldManager::new();
        fm.register_osc133_region(5, 10, "a".to_string(), Some(0));
        fm.register_osc133_region(10, 15, "b".to_string(), Some(0));
        fm.register_osc133_region(20, 30, "c".to_string(), Some(0));
        fm.toggle_fold(5);
        fm.toggle_fold(10);
        fm.toggle_fold(20);
        let layout = fm.build_layout(40, 5, 0);
        for actual in [0u32, 4, 5, 9, 10, 14, 15, 19, 20, 29, 30, 50, 100] {
            assert_eq!(
                layout.actual_line_to_display(actual),
                fm.actual_line_to_display(actual),
                "actual {actual} mismatch between layout and manager"
            );
        }
    }

    // ── build_layout / FoldLayout ────────────────────────────

    #[test]
    fn build_layout_no_collapse_is_identity_window() {
        // No collapsed region: every screen row maps to a linear buffer row
        // starting at display_start (= total - viewport - offset).
        let mut fm = FoldManager::new();
        fm.register_osc133_region(2, 5, "test".to_string(), Some(0));
        // scrollback_len = 10, viewport = 4, offset = 0.
        // total_display = 14 (nothing hidden), display_start = 14 - 4 - 0 = 10.
        let layout = fm.build_layout(10, 4, 0);
        assert_eq!(layout.display_start, 10);
        assert_eq!(layout.rows.len(), 4);
        for (r, kind) in layout.rows.iter().enumerate() {
            match kind {
                FoldRowKind::Cells { actual_line } => assert_eq!(*actual_line, 10 + r as u32),
                FoldRowKind::Summary { .. } => panic!("no summary expected"),
            }
        }
    }

    #[test]
    fn build_layout_collapsed_region_marks_summary_and_skips_body() {
        // Region 2..6 (4 rows) collapsed → hides 3 body rows. With
        // scrollback_len = 10, viewport = 8, offset large enough to show
        // from the top, the summary sits at display line 2.
        let mut fm = FoldManager::new();
        fm.register_osc133_region(2, 6, "ls".to_string(), Some(0));
        fm.toggle_fold(2);
        // total_actual = 18, hidden = 3, total_display = 15.
        // Pick offset so display_start = 0: offset = 15 - 8 = 7.
        let layout = fm.build_layout(10, 8, 7);
        assert_eq!(layout.display_start, 0);
        assert_eq!(layout.rows.len(), 8);
        // Display lines 0,1 = actual 0,1.
        assert_eq!(layout.rows[0], FoldRowKind::Cells { actual_line: 0 });
        assert_eq!(layout.rows[1], FoldRowKind::Cells { actual_line: 1 });
        // Display line 2 = summary for region 2..6.
        match &layout.rows[2] {
            FoldRowKind::Summary { region } => {
                assert_eq!(region.start_line, 2);
                assert_eq!(region.command_text.as_deref(), Some("ls"));
            }
            other => panic!("expected summary, got {other:?}"),
        }
        // Display line 3 = actual 6 (first row after the hidden body).
        assert_eq!(layout.rows[3], FoldRowKind::Cells { actual_line: 6 });
        assert_eq!(layout.rows[4], FoldRowKind::Cells { actual_line: 7 });
    }

    #[test]
    fn build_layout_display_start_saturates_at_zero() {
        // A scroll_offset larger than the content keeps display_start at 0
        // (saturating) rather than underflowing.
        let mut fm = FoldManager::new();
        fm.register_osc133_region(2, 6, "x".to_string(), Some(0));
        fm.toggle_fold(2);
        let layout = fm.build_layout(10, 8, 9999);
        assert_eq!(layout.display_start, 0);
    }

    #[test]
    fn fold_layout_region_at_line_only_collapsed() {
        let mut fm = FoldManager::new();
        fm.register_osc133_region(5, 15, "test".to_string(), Some(0));
        fm.toggle_fold(5);
        let layout = fm.build_layout(20, 5, 0);
        // Inside the collapsed body → Some.
        assert!(layout.region_at_line(5).is_some());
        assert!(layout.region_at_line(14).is_some());
        // Outside → None.
        assert!(layout.region_at_line(4).is_none());
        assert!(layout.region_at_line(15).is_none());
    }

    #[test]
    fn fold_layout_region_at_line_excludes_expanded() {
        // An expanded region is not in the collapsed snapshot, so the
        // layout reports no region there (search must NOT skip its matches).
        let mut fm = FoldManager::new();
        fm.register_osc133_region(5, 15, "test".to_string(), Some(0));
        // Not collapsed.
        let layout = fm.build_layout(20, 5, 0);
        assert!(layout.region_at_line(10).is_none());
    }

    #[test]
    fn fold_layout_actual_to_display_matches_manager() {
        // The immutable FoldLayout mapping agrees with FoldManager's.
        let mut fm = FoldManager::new();
        fm.register_osc133_region(5, 15, "first".to_string(), Some(0));
        fm.register_osc133_region(25, 35, "second".to_string(), Some(1));
        fm.toggle_fold(5);
        fm.toggle_fold(25);
        let layout = fm.build_layout(40, 5, 0);
        for actual in [0u32, 4, 5, 10, 15, 16, 24, 25, 30, 35, 36, 100] {
            assert_eq!(
                layout.actual_line_to_display(actual),
                fm.actual_line_to_display(actual),
                "actual {actual} mismatch"
            );
        }
    }
}
