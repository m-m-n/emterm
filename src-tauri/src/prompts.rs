//! Prompt-mark tracking for OSC 133 semantic prompts (prompt-to-prompt
//! navigation).
//!
//! Port of the WebView build's `src/terminal/semantic-zone.ts`
//! (`SemanticZoneTracker`). `term_core` captures OSC 133 sub-types during
//! `process_pty_data`, stamping each with the absolute row it was emitted
//! on (`scrollback_len + cursor.row`). `Tab::pump` drains them via
//! `TerminalCore::take_prompt_marks` under the core lock, normalizes each
//! row for any scrollback eviction that happened after the mark was
//! captured, and pushes the resolved mark here.
//! [`crate::app::App::jump_to_prompt`] then queries
//! [`PromptTracker::find_prev_prompt`] / [`PromptTracker::find_next_prompt`]
//! to scroll between prompts.
//!
//! Row coordinates are absolute in the scrollback frame: `0..scrollback_len`
//! addresses scrollback rows (oldest first), `scrollback_len..scrollback_len
//! + rows` addresses the live viewport. This matches the WebView `lineIndex`
//! (`scrollbackLength + cursor.row`). When scrollback evicts its oldest rows
//! the whole frame shifts down, so [`PromptTracker::prune_before_line`]
//! drops out-of-range marks and re-bases the survivors — mirroring the
//! WebView `pruneBeforeLine`.

use std::collections::VecDeque;

/// OSC 133 semantic-prompt sub-type.
///
/// See <https://gitlab.freedesktop.org/Per_Bothner/specifications/-/blob/master/proposals/semantic-prompts.md>.
/// Lives here (rather than in `callbacks`) because the prompt-mark pipeline
/// is the only consumer now that `term_core` owns OSC 133 capture; the
/// callback path no longer materializes marks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PromptMarkKind {
    /// `A` — prompt start.
    PromptStart,
    /// `B` — command start (user input begins).
    CommandStart,
    /// `C` — command exec (user input ends, command runs).
    CommandExec,
    /// `D` — command end (exit-code-bearing).
    CommandEnd,
}

impl PromptMarkKind {
    /// Map a raw OSC 133 sub-type byte (as captured by
    /// `term_core`'s `PendingPromptMark::kind`) to a [`PromptMarkKind`].
    /// `term_core` only ever stores `A`/`B`/`C`/`D`, so unknown bytes
    /// (which never reach the tracker) map to `None`.
    pub fn from_byte(b: u8) -> Option<Self> {
        match b {
            b'A' => Some(Self::PromptStart),
            b'B' => Some(Self::CommandStart),
            b'C' => Some(Self::CommandExec),
            b'D' => Some(Self::CommandEnd),
            _ => None,
        }
    }
}

/// Upper bound on the number of resolved marks the tracker retains.
///
/// `10_000` is the default `scrollback_lines`; each scrollback line can in
/// principle carry one of the four OSC 133 sub-types (`A`/`B`/`C`/`D`),
/// giving `10_000 × 4 = 40_000` as a generous ceiling. When exceeded the
/// oldest mark is dropped (`pop_front`). With this cap in place
/// `prune_before_line`'s full scan is bounded — answering review F6.
const MAX_MARKS: usize = 10_000 * 4;

/// A prompt mark whose `row` has been resolved against the core. The row is
/// the absolute scrollback-frame line the mark was received on, normalized
/// for any eviction that occurred between capture and drain.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedPromptMark {
    /// OSC 133 sub-type (`A`/`B`/`C`/`D`). Only `PromptStart` (`A`) is a
    /// navigation target; the others are retained for parity with the
    /// WebView tracker (which stores all four) and possible future use.
    pub kind: PromptMarkKind,
    /// Absolute row in the scrollback frame: `0..scrollback_len` is
    /// scrollback (oldest first), `scrollback_len..+rows` is the viewport.
    pub row: u32,
    /// Optional exit code attached to a `CommandEnd` (`D`) mark.
    pub exit_code: Option<i32>,
}

/// Holds resolved OSC 133 marks in arrival order and answers prompt-jump
/// queries. Port of `SemanticZoneTracker`.
#[derive(Debug, Default)]
pub struct PromptTracker {
    /// Marks in arrival order. In practice rows are non-decreasing (the
    /// shell emits marks as the cursor advances), but the search methods
    /// scan linearly and so do not depend on strict ordering — matching
    /// the WebView implementation. A `VecDeque` so the oldest mark can be
    /// dropped in O(1) when [`MAX_MARKS`] is exceeded.
    marks: VecDeque<ResolvedPromptMark>,
}

impl PromptTracker {
    /// Record a resolved mark. Unknown kinds are never produced here (the
    /// OSC parser already filtered them), so every pushed mark is kept —
    /// the `A`-only filtering happens at query time. When the tracker is at
    /// [`MAX_MARKS`] the oldest mark is evicted first so an OSC 133 flood
    /// (the PTY is a trust boundary) cannot grow it without bound.
    pub fn push(&mut self, mark: ResolvedPromptMark) {
        if self.marks.len() >= MAX_MARKS {
            self.marks.pop_front();
        }
        self.marks.push_back(mark);
    }

    /// Row of the nearest prompt-start (`A`) mark *above* `current_line`,
    /// i.e. the last `A` mark with `row < current_line`. Rows equal to
    /// `current_line` are excluded (WebView `findPrevPrompt`).
    pub fn find_prev_prompt(&self, current_line: u32) -> Option<u32> {
        self.marks
            .iter()
            .rev()
            .find(|m| m.kind == PromptMarkKind::PromptStart && m.row < current_line)
            .map(|m| m.row)
    }

    /// Row of the nearest prompt-start (`A`) mark *below* `current_line`,
    /// i.e. the first `A` mark with `row > current_line`. Rows equal to
    /// `current_line` are excluded (WebView `findNextPrompt`).
    pub fn find_next_prompt(&self, current_line: u32) -> Option<u32> {
        self.marks
            .iter()
            .find(|m| m.kind == PromptMarkKind::PromptStart && m.row > current_line)
            .map(|m| m.row)
    }

    /// All resolved marks in arrival order. Mirrors the WebView
    /// `SemanticZoneTracker.getMarkers()`. The OSC 133 fold-region builder
    /// (`Tab::backfill_prompt_marks`) scans this in reverse to pair a `D`
    /// mark with its preceding `C` (and the `B` before that), exactly as the
    /// WebView `registerOsc133FoldRegion` walks `getMarkers()`. Rows are in
    /// the same post-prune absolute frame the search/fold code expects.
    pub fn marks(&self) -> &VecDeque<ResolvedPromptMark> {
        &self.marks
    }

    /// Drop marks whose row is below `count` and re-base the survivors by
    /// subtracting `count`. Called when `count` oldest scrollback rows were
    /// evicted, shifting the whole frame down (WebView `pruneBeforeLine`).
    pub fn prune_before_line(&mut self, count: u32) {
        if count == 0 {
            return;
        }
        self.marks.retain(|m| m.row >= count);
        for m in &mut self.marks {
            m.row -= count;
        }
    }

    /// Discard all marks (mux snapshot / restore re-baseline).
    pub fn clear(&mut self) {
        self.marks.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mark(kind: PromptMarkKind, row: u32) -> ResolvedPromptMark {
        ResolvedPromptMark {
            kind,
            row,
            exit_code: None,
        }
    }

    fn prompt(row: u32) -> ResolvedPromptMark {
        mark(PromptMarkKind::PromptStart, row)
    }

    #[test]
    fn find_prev_returns_last_a_below_current() {
        let mut t = PromptTracker::default();
        t.push(prompt(2));
        t.push(prompt(5));
        t.push(prompt(10));
        // current_line = 8 → the last A strictly below is row 5.
        assert_eq!(t.find_prev_prompt(8), Some(5));
    }

    #[test]
    fn find_prev_excludes_equal_row() {
        let mut t = PromptTracker::default();
        t.push(prompt(5));
        // current_line == 5 → row 5 is not "above"; no match.
        assert_eq!(t.find_prev_prompt(5), None);
    }

    #[test]
    fn find_next_returns_first_a_above_current() {
        let mut t = PromptTracker::default();
        t.push(prompt(2));
        t.push(prompt(5));
        t.push(prompt(10));
        // current_line = 5 → first A strictly above is row 10.
        assert_eq!(t.find_next_prompt(5), Some(10));
    }

    #[test]
    fn find_next_excludes_equal_row() {
        let mut t = PromptTracker::default();
        t.push(prompt(7));
        assert_eq!(t.find_next_prompt(7), None);
    }

    #[test]
    fn non_a_kinds_are_not_search_targets() {
        let mut t = PromptTracker::default();
        t.push(mark(PromptMarkKind::CommandStart, 1));
        t.push(mark(PromptMarkKind::CommandExec, 2));
        t.push(mark(PromptMarkKind::CommandEnd, 3));
        // None of B/C/D should be returned by either search.
        assert_eq!(t.find_prev_prompt(10), None);
        assert_eq!(t.find_next_prompt(0), None);
    }

    #[test]
    fn find_prev_skips_non_a_to_reach_a() {
        let mut t = PromptTracker::default();
        t.push(prompt(2));
        t.push(mark(PromptMarkKind::CommandStart, 4));
        t.push(mark(PromptMarkKind::CommandEnd, 6));
        // current_line = 8: the only A below is row 2 (B/C/D ignored).
        assert_eq!(t.find_prev_prompt(8), Some(2));
    }

    #[test]
    fn prune_drops_below_and_shifts_survivors() {
        let mut t = PromptTracker::default();
        t.push(prompt(1));
        t.push(prompt(3));
        t.push(prompt(6));
        // Evict 3 oldest rows: row 1 drops; 3→0, 6→3.
        t.prune_before_line(3);
        // Survivors are now at rows 0 and 3.
        assert_eq!(t.find_prev_prompt(10), Some(3));
        assert_eq!(t.find_next_prompt(0), Some(3));
        assert_eq!(t.find_prev_prompt(1), Some(0));
    }

    #[test]
    fn prune_zero_is_noop() {
        let mut t = PromptTracker::default();
        t.push(prompt(2));
        t.prune_before_line(0);
        assert_eq!(t.find_prev_prompt(5), Some(2));
    }

    #[test]
    fn clear_removes_all_marks() {
        let mut t = PromptTracker::default();
        t.push(prompt(2));
        t.push(prompt(4));
        t.clear();
        assert_eq!(t.find_prev_prompt(10), None);
        assert_eq!(t.find_next_prompt(0), None);
    }

    #[test]
    fn push_evicts_oldest_when_at_cap() {
        let mut t = PromptTracker::default();
        // Fill exactly to the cap with ascending rows 0..MAX_MARKS.
        for r in 0..MAX_MARKS as u32 {
            t.push(prompt(r));
        }
        assert_eq!(t.marks.len(), MAX_MARKS);
        // Oldest mark (row 0) is still present at the cap boundary.
        assert_eq!(t.marks.front().map(|m| m.row), Some(0));
        // One more push evicts the oldest (row 0) and appends a new tail.
        t.push(prompt(MAX_MARKS as u32));
        assert_eq!(t.marks.len(), MAX_MARKS, "stays bounded at cap");
        assert_eq!(t.marks.front().map(|m| m.row), Some(1), "row 0 evicted");
        assert_eq!(
            t.marks.back().map(|m| m.row),
            Some(MAX_MARKS as u32),
            "new mark appended"
        );
    }

    #[test]
    fn push_just_below_cap_does_not_evict() {
        let mut t = PromptTracker::default();
        for r in 0..(MAX_MARKS as u32 - 1) {
            t.push(prompt(r));
        }
        assert_eq!(t.marks.len(), MAX_MARKS - 1);
        // The oldest mark is retained because we are one below the cap.
        assert_eq!(t.marks.front().map(|m| m.row), Some(0));
    }

    #[test]
    fn from_byte_maps_known_kinds() {
        assert_eq!(
            PromptMarkKind::from_byte(b'A'),
            Some(PromptMarkKind::PromptStart)
        );
        assert_eq!(
            PromptMarkKind::from_byte(b'B'),
            Some(PromptMarkKind::CommandStart)
        );
        assert_eq!(
            PromptMarkKind::from_byte(b'C'),
            Some(PromptMarkKind::CommandExec)
        );
        assert_eq!(
            PromptMarkKind::from_byte(b'D'),
            Some(PromptMarkKind::CommandEnd)
        );
        assert_eq!(PromptMarkKind::from_byte(b'Z'), None);
    }
}
