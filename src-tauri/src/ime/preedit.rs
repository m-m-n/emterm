//! IME preedit state.
//!
//! Holds the in-progress composition string from the platform IME along
//! with the anchor cell (the cursor position at the moment the
//! composition started). The renderer reads this state to draw an
//! underline overlay beneath the cursor.
//!
//! The sanitization helper drops C0 (`0x00..=0x1F` except `\t` and `\n`)
//! and C1 (`0x80..=0x9F`) bytes so a malformed IME payload cannot inject
//! a control sequence into either the rendering pipeline or the PTY.
//! The same helper is shared with [`crate::ime::commit::write_commit`]
//! so the displayed preedit text and the bytes ultimately pushed to the
//! shell agree on what is allowed.

/// Anchor for the preedit overlay: the cursor cell the composition
/// started on. Stored as (row, col) in grid coordinates.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Anchor {
    pub row: u16,
    pub col: u16,
}

/// Per-tab preedit composition state.
///
/// Default-initialized state is "inactive" (empty text, anchor (0,0)).
/// Use [`State::active`] to check whether the renderer should draw an
/// overlay.
#[derive(Debug, Default, Clone)]
pub struct State {
    text: String,
    anchor: Anchor,
}

impl State {
    /// Replace the composition text and re-anchor. Called from the
    /// `egui::Event::Ime(ImeEvent::Preedit(_))` route.
    pub fn set(&mut self, preedit_text: &str, anchor: Anchor) {
        self.text = sanitize(preedit_text);
        self.anchor = anchor;
    }

    /// Wipe the composition (called on commit, focus loss, or PTY close).
    pub fn clear(&mut self) {
        self.text.clear();
        self.anchor = Anchor::default();
    }

    /// `true` when there is a non-empty composition that the renderer
    /// should draw.
    pub fn active(&self) -> bool {
        !self.text.is_empty()
    }

    /// Sanitized composition text. Always rendering-safe — no C0/C1
    /// control bytes survive sanitization.
    pub fn text(&self) -> &str {
        &self.text
    }

    /// Anchor cell (cursor position at composition start).
    pub fn anchor(&self) -> Anchor {
        self.anchor
    }
}

/// Drop C0 (`0x00..=0x1F` except `\t` and `\n`) and C1 (`0x80..=0x9F`)
/// control bytes from `input`. Kept `pub(crate)` so the commit path can
/// reuse it without exposing it as public API.
///
/// We operate on `char` rather than raw bytes because the input is a
/// UTF-8 string from egui — sanitizing per-byte would corrupt multi-byte
/// characters.
pub(crate) fn sanitize(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for c in input.chars() {
        let cp = c as u32;
        // Allow \t and \n; drop the rest of C0 (0x00..=0x1F including DEL 0x7F).
        if c == '\t' || c == '\n' {
            out.push(c);
            continue;
        }
        if cp <= 0x1F {
            continue;
        }
        if cp == 0x7F {
            // DEL — strictly speaking C0; drop it for the same reason.
            continue;
        }
        if (0x80..=0x9F).contains(&cp) {
            continue;
        }
        out.push(c);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── Sanitize ─────────────────────────────────────────────────────

    #[test]
    fn sanitize_passes_plain_ascii() {
        assert_eq!(sanitize("hello"), "hello");
    }

    #[test]
    fn sanitize_passes_cjk() {
        // Hiragana あ (U+3042) — well outside C0/C1 — must survive.
        assert_eq!(sanitize("あい"), "あい");
    }

    #[test]
    fn sanitize_passes_tab_and_newline() {
        // Explicit exemption from C0 drop.
        assert_eq!(sanitize("a\tb\nc"), "a\tb\nc");
    }

    #[test]
    fn sanitize_drops_c0_controls() {
        // BEL (0x07), CR (0x0D), ESC (0x1B), and DEL (0x7F) must all
        // be dropped.
        let input = "a\x07b\x0Dc\x1Bd\x7Fe";
        assert_eq!(sanitize(input), "abcde");
    }

    #[test]
    fn sanitize_drops_c1_controls() {
        // C1 range is U+0080..=U+009F. Pick a couple of representative
        // codepoints: PAD (0x80), CSI (0x9B), APC (0x9F).
        let input = "x\u{0080}y\u{009B}z\u{009F}w";
        assert_eq!(sanitize(input), "xyzw");
    }

    #[test]
    fn sanitize_drops_null_byte() {
        assert_eq!(sanitize("a\x00b"), "ab");
    }

    #[test]
    fn sanitize_keeps_higher_codepoints() {
        // 0xA0 (no-break space) is the first codepoint *after* C1.
        assert_eq!(sanitize("a\u{00A0}b"), "a\u{00A0}b");
    }

    #[test]
    fn sanitize_empty_string_is_empty() {
        assert_eq!(sanitize(""), "");
    }

    // ── State ────────────────────────────────────────────────────────

    #[test]
    fn default_state_is_inactive() {
        let s = State::default();
        assert!(!s.active());
        assert_eq!(s.text(), "");
        assert_eq!(s.anchor(), Anchor::default());
    }

    #[test]
    fn set_marks_state_active_and_sanitizes() {
        let mut s = State::default();
        s.set("ab\x07c", Anchor { row: 3, col: 7 });
        assert!(s.active());
        assert_eq!(s.text(), "abc");
        assert_eq!(s.anchor(), Anchor { row: 3, col: 7 });
    }

    #[test]
    fn set_replaces_previous_text_and_anchor() {
        let mut s = State::default();
        s.set("first", Anchor { row: 0, col: 0 });
        s.set("second", Anchor { row: 5, col: 2 });
        assert_eq!(s.text(), "second");
        assert_eq!(s.anchor(), Anchor { row: 5, col: 2 });
    }

    #[test]
    fn clear_wipes_text_and_anchor() {
        let mut s = State::default();
        s.set("xyz", Anchor { row: 1, col: 1 });
        s.clear();
        assert!(!s.active());
        assert_eq!(s.text(), "");
        assert_eq!(s.anchor(), Anchor::default());
    }

    #[test]
    fn set_empty_string_is_inactive() {
        // Some IMEs emit a Preedit("") to signal "no candidate, but
        // composition still open". We treat that as inactive for
        // rendering purposes — the overlay would be zero-width anyway.
        let mut s = State::default();
        s.set("", Anchor { row: 2, col: 4 });
        assert!(!s.active());
        // Anchor is still recorded so the next non-empty set lands at
        // the right cell. (Documented behavior, not strictly required.)
        assert_eq!(s.anchor(), Anchor { row: 2, col: 4 });
    }

    #[test]
    fn set_after_clear_round_trips() {
        let mut s = State::default();
        s.set("abc", Anchor { row: 1, col: 2 });
        s.clear();
        s.set("def", Anchor { row: 3, col: 4 });
        assert!(s.active());
        assert_eq!(s.text(), "def");
        assert_eq!(s.anchor(), Anchor { row: 3, col: 4 });
    }

    // TS-ime-3: both directions (preedit + commit) must use the same
    // sanitizer so the user never types something that displays
    // differently from what hits the shell.
    #[test]
    fn sanitize_helper_shared_with_commit_path() {
        // Direct comparison: the commit module re-uses this exact
        // function. Keeping a test here pins the contract.
        let payload = "raw\x1bbody";
        let preedit_view = sanitize(payload);
        let commit_view = crate::ime::commit::sanitize_for_test(payload);
        assert_eq!(preedit_view, commit_view);
        assert_eq!(preedit_view, "rawbody");
    }
}
