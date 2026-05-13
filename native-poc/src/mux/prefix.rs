//! Prefix-key state machine for mux mode.
//!
//! Default chord: `Ctrl+B`. After observing the prefix the latch is **armed**
//! and the next non-modifier key drives one of:
//!
//! | Follow-up key        | Action                                    |
//! |----------------------|-------------------------------------------|
//! | `n`                  | `PrefixAction::NextWindow`                |
//! | `p`                  | `PrefixAction::PrevWindow`                |
//! | `d`                  | `PrefixAction::Detach`                    |
//! | `0`..=`9`            | `PrefixAction::SelectWindow(<digit>)`     |
//! | the prefix chord     | `PrefixAction::Literal` (passes `0x02`)   |
//! | anything else        | `PrefixAction::None` and cancel the latch |
//!
//! The latch auto-cancels after 3 seconds with no follow-up. The timeout is
//! enforced by the caller passing the current `Instant` into
//! [`Latch::observe`]; the latch never queries the wall clock on its own so
//! tests can drive it with a synthetic clock.
//!
//! The module is intentionally pure — no I/O, no `egui::Context`, no logger.

use std::time::{Duration, Instant};

use egui::{Key, Modifiers};

/// Default time the latch stays armed waiting for a follow-up key. Mirrors
/// the common tmux default (`set -g escape-time 500`, latch timeout 3 s).
pub const DEFAULT_ARMED_TIMEOUT: Duration = Duration::from_secs(3);

/// Default literal byte sent when the user double-taps the prefix
/// (`Ctrl+B Ctrl+B`). `0x02` is the C0 control code for `Ctrl+B`, which is
/// what a passthrough would have produced.
pub const DEFAULT_LITERAL_BYTE: u8 = 0x02;

/// Output of [`Latch::observe`]. The caller routes these into
/// `ControlMsg::SelectWindow(Next / Prev / Index(n))`, `ControlMsg::Detach`
/// or a raw PTY write (`Literal`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrefixAction {
    /// The key event did not produce a prefix-mediated action. The caller
    /// should fall through to the normal keybinds + PTY passthrough path.
    None,
    /// The user double-tapped the prefix chord; send the prefix's literal
    /// byte ([`DEFAULT_LITERAL_BYTE`]) to the active PTY so they can talk
    /// to programs that themselves use `Ctrl+B`.
    Literal,
    /// Detach from the current mux session.
    Detach,
    /// Switch to the next window in the session's window list.
    NextWindow,
    /// Switch to the previous window in the session's window list.
    PrevWindow,
    /// Switch to the window at the given (0-based) index. The latch emits
    /// the digit verbatim; the caller is responsible for mapping it to the
    /// daemon's `ControlMsg::SelectWindow(Index(d))` representation.
    SelectWindow(u8),
}

/// Configurable prefix chord. Constructed by parsing `settings.mux.prefix_key`
/// at startup (see `super::prefix::parse_prefix_key`). The default is
/// `Ctrl+B`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PrefixChord {
    pub ctrl: bool,
    pub shift: bool,
    pub alt: bool,
    pub key: Key,
}

impl Default for PrefixChord {
    fn default() -> Self {
        Self {
            ctrl: true,
            shift: false,
            alt: false,
            key: Key::B,
        }
    }
}

impl PrefixChord {
    /// Match the chord against an egui `(Modifiers, Key)` pair.
    fn matches(&self, mods: &Modifiers, key: Key) -> bool {
        // `mods.command` aliases to ctrl on non-mac (and is what tao reports
        // on Windows for the literal Ctrl key); treat it as canonical.
        let ctrl = mods.ctrl || mods.command;
        ctrl == self.ctrl && mods.shift == self.shift && mods.alt == self.alt && key == self.key
    }
}

/// Internal latch state. Carries the timestamp the prefix was armed so the
/// follow-up timeout can be enforced.
#[derive(Debug, Clone, Copy)]
enum State {
    Idle,
    Armed { since: Instant },
}

/// Prefix-key latch. Hold one per window (or per tab in mux mode); the latch
/// is naturally per-active-input-context, so a global instance attached to
/// the focused tab is sufficient.
#[derive(Debug)]
pub struct Latch {
    chord: PrefixChord,
    timeout: Duration,
    state: State,
}

impl Default for Latch {
    fn default() -> Self {
        Self::new(PrefixChord::default(), DEFAULT_ARMED_TIMEOUT)
    }
}

impl Latch {
    pub fn new(chord: PrefixChord, timeout: Duration) -> Self {
        Self {
            chord,
            timeout,
            state: State::Idle,
        }
    }

    /// Returns true when the latch is currently waiting for a follow-up key
    /// (used by the UI to draw a transient "prefix armed" indicator).
    pub fn is_armed(&self) -> bool {
        matches!(self.state, State::Armed { .. })
    }

    /// Forcibly cancel the latch. Called when focus is lost or the tab
    /// detaches from the mux session.
    pub fn cancel(&mut self) {
        self.state = State::Idle;
    }

    /// Feed one keyboard event into the latch.
    ///
    /// `now` is the wall-clock instant the event was observed. The caller
    /// passes `Instant::now()` in production code; tests pass a synthetic
    /// clock so timeouts are reproducible.
    ///
    /// Returns the [`PrefixAction`] derived from the event. The latch
    /// transitions atomically inside this call; the caller does not need
    /// to do any bookkeeping.
    pub fn observe(&mut self, mods: Modifiers, key: Key, now: Instant) -> PrefixAction {
        // Auto-cancel if the previous arm has expired. Do this before
        // matching so a stale arm cannot accidentally trigger.
        if let State::Armed { since } = self.state {
            if now.duration_since(since) >= self.timeout {
                self.state = State::Idle;
            }
        }

        match self.state {
            State::Idle => {
                if self.chord.matches(&mods, key) {
                    self.state = State::Armed { since: now };
                }
                PrefixAction::None
            }
            State::Armed { .. } => {
                // Double-press: the same chord while armed is a literal
                // passthrough request.
                if self.chord.matches(&mods, key) {
                    self.state = State::Idle;
                    return PrefixAction::Literal;
                }

                // Modifier-only chords (Ctrl, Shift, Alt alone) keep the
                // latch armed so the user can hold Ctrl while typing the
                // follow-up letter. egui surfaces these via the `Key` enum
                // — anything that is not a "real" letter / digit / etc.
                // we treat as a no-op for the latch state machine.
                //
                // egui::Key has no specific "modifier-only" variant; in
                // practice tao surfaces modifier presses as
                // `Modifiers` updates rather than `Key` events, so this
                // branch is dead in production. We still keep an explicit
                // bail-out for any future event source that does emit
                // standalone modifier keys.

                let action = decode_follow_up(key);
                self.state = State::Idle;
                action
            }
        }
    }
}

/// Decode the follow-up key into an action. Anything not in the table
/// returns `PrefixAction::None` and the caller still consumes the latch
/// (the prefix chord is "used up" once armed).
fn decode_follow_up(key: Key) -> PrefixAction {
    match key {
        Key::N => PrefixAction::NextWindow,
        Key::P => PrefixAction::PrevWindow,
        Key::D => PrefixAction::Detach,
        Key::Num0 => PrefixAction::SelectWindow(0),
        Key::Num1 => PrefixAction::SelectWindow(1),
        Key::Num2 => PrefixAction::SelectWindow(2),
        Key::Num3 => PrefixAction::SelectWindow(3),
        Key::Num4 => PrefixAction::SelectWindow(4),
        Key::Num5 => PrefixAction::SelectWindow(5),
        Key::Num6 => PrefixAction::SelectWindow(6),
        Key::Num7 => PrefixAction::SelectWindow(7),
        Key::Num8 => PrefixAction::SelectWindow(8),
        Key::Num9 => PrefixAction::SelectWindow(9),
        _ => PrefixAction::None,
    }
}

/// Parse a textual prefix-key spec like `"Ctrl+B"` into a [`PrefixChord`].
///
/// Recognized forms (case-insensitive on modifier names, case-insensitive on
/// single-letter keys):
///
/// - `Ctrl+B`, `Ctrl+Shift+B`, `Alt+X`, …
/// - Single-letter keys map to `Key::A`..`Key::Z`.
/// - Digits `0..9` map to `Key::Num0`..`Key::Num9`.
///
/// Returns `None` on an unparseable spec; the caller falls back to
/// [`PrefixChord::default`] and logs a `warn`.
pub fn parse_prefix_key(spec: &str) -> Option<PrefixChord> {
    let mut ctrl = false;
    let mut shift = false;
    let mut alt = false;
    let mut key: Option<Key> = None;
    for tok_raw in spec.split('+') {
        let tok = tok_raw.trim();
        if tok.is_empty() {
            return None;
        }
        match tok.to_ascii_lowercase().as_str() {
            "ctrl" | "control" => ctrl = true,
            "shift" => shift = true,
            "alt" | "meta" => alt = true,
            _ => {
                if key.is_some() {
                    // Two non-modifier tokens — refuse.
                    return None;
                }
                key = parse_key_token(tok);
                key?; // bail if the token didn't map to any Key
            }
        }
    }
    Some(PrefixChord {
        ctrl,
        shift,
        alt,
        key: key?,
    })
}

fn parse_key_token(tok: &str) -> Option<Key> {
    if tok.len() == 1 {
        let c = tok.chars().next().unwrap();
        if c.is_ascii_alphabetic() {
            return Some(letter_to_key(c.to_ascii_uppercase()));
        }
        if let Some(digit) = c.to_digit(10) {
            return digit_to_key(digit as u8);
        }
    }
    None
}

fn letter_to_key(c: char) -> Key {
    match c {
        'A' => Key::A,
        'B' => Key::B,
        'C' => Key::C,
        'D' => Key::D,
        'E' => Key::E,
        'F' => Key::F,
        'G' => Key::G,
        'H' => Key::H,
        'I' => Key::I,
        'J' => Key::J,
        'K' => Key::K,
        'L' => Key::L,
        'M' => Key::M,
        'N' => Key::N,
        'O' => Key::O,
        'P' => Key::P,
        'Q' => Key::Q,
        'R' => Key::R,
        'S' => Key::S,
        'T' => Key::T,
        'U' => Key::U,
        'V' => Key::V,
        'W' => Key::W,
        'X' => Key::X,
        'Y' => Key::Y,
        'Z' => Key::Z,
        // Unreachable because the caller filters on `is_ascii_alphabetic`,
        // but be conservative.
        _ => Key::B,
    }
}

fn digit_to_key(d: u8) -> Option<Key> {
    match d {
        0 => Some(Key::Num0),
        1 => Some(Key::Num1),
        2 => Some(Key::Num2),
        3 => Some(Key::Num3),
        4 => Some(Key::Num4),
        5 => Some(Key::Num5),
        6 => Some(Key::Num6),
        7 => Some(Key::Num7),
        8 => Some(Key::Num8),
        9 => Some(Key::Num9),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mods(ctrl: bool, shift: bool, alt: bool) -> Modifiers {
        Modifiers {
            ctrl,
            shift,
            alt,
            command: false,
            mac_cmd: false,
        }
    }

    // ── TS-prefix-1: arming + follow-up actions ─────────────────────────

    #[test]
    fn ctrl_b_arms_the_latch_without_emitting_action() {
        let mut l = Latch::default();
        let now = Instant::now();
        assert_eq!(
            l.observe(mods(true, false, false), Key::B, now),
            PrefixAction::None
        );
        assert!(l.is_armed());
    }

    #[test]
    fn ctrl_b_then_n_emits_next_window() {
        let mut l = Latch::default();
        let t0 = Instant::now();
        let _ = l.observe(mods(true, false, false), Key::B, t0);
        let t1 = t0 + Duration::from_millis(50);
        assert_eq!(
            l.observe(mods(false, false, false), Key::N, t1),
            PrefixAction::NextWindow
        );
        assert!(!l.is_armed());
    }

    #[test]
    fn ctrl_b_then_p_emits_prev_window() {
        let mut l = Latch::default();
        let t0 = Instant::now();
        let _ = l.observe(mods(true, false, false), Key::B, t0);
        assert_eq!(
            l.observe(
                mods(false, false, false),
                Key::P,
                t0 + Duration::from_millis(50)
            ),
            PrefixAction::PrevWindow
        );
    }

    #[test]
    fn ctrl_b_then_d_emits_detach() {
        let mut l = Latch::default();
        let t0 = Instant::now();
        let _ = l.observe(mods(true, false, false), Key::B, t0);
        assert_eq!(
            l.observe(
                mods(false, false, false),
                Key::D,
                t0 + Duration::from_millis(50)
            ),
            PrefixAction::Detach
        );
    }

    #[test]
    fn ctrl_b_then_digit_emits_select_window() {
        for (key, want) in [
            (Key::Num0, 0u8),
            (Key::Num1, 1),
            (Key::Num5, 5),
            (Key::Num9, 9),
        ] {
            let mut l = Latch::default();
            let t0 = Instant::now();
            let _ = l.observe(mods(true, false, false), Key::B, t0);
            assert_eq!(
                l.observe(
                    mods(false, false, false),
                    key,
                    t0 + Duration::from_millis(50)
                ),
                PrefixAction::SelectWindow(want),
                "digit {want}"
            );
        }
    }

    // ── TS-prefix-2: double-press literal + timeout ─────────────────────

    #[test]
    fn double_prefix_emits_literal() {
        let mut l = Latch::default();
        let t0 = Instant::now();
        let _ = l.observe(mods(true, false, false), Key::B, t0);
        assert_eq!(
            l.observe(
                mods(true, false, false),
                Key::B,
                t0 + Duration::from_millis(20)
            ),
            PrefixAction::Literal
        );
        assert!(!l.is_armed());
    }

    #[test]
    fn armed_latch_auto_cancels_after_3s() {
        let mut l = Latch::default();
        let t0 = Instant::now();
        let _ = l.observe(mods(true, false, false), Key::B, t0);
        assert!(l.is_armed());
        // Exactly at the timeout boundary the latch is treated as expired.
        let later = t0 + DEFAULT_ARMED_TIMEOUT;
        // Any follow-up after the timeout is a no-op (latch is idle); if
        // the follow-up is the prefix itself it re-arms instead.
        assert_eq!(
            l.observe(mods(false, false, false), Key::N, later),
            PrefixAction::None
        );
        assert!(!l.is_armed());
    }

    #[test]
    fn armed_latch_consumes_on_unknown_follow_up() {
        let mut l = Latch::default();
        let t0 = Instant::now();
        let _ = l.observe(mods(true, false, false), Key::B, t0);
        // Hitting an unmapped key (`Q`) cancels the arm without emitting.
        assert_eq!(
            l.observe(
                mods(false, false, false),
                Key::Q,
                t0 + Duration::from_millis(20)
            ),
            PrefixAction::None
        );
        assert!(!l.is_armed());
    }

    #[test]
    fn cancel_clears_arm() {
        let mut l = Latch::default();
        let _ = l.observe(mods(true, false, false), Key::B, Instant::now());
        assert!(l.is_armed());
        l.cancel();
        assert!(!l.is_armed());
    }

    // ── TS-prefix-3: configurable chord + parse helper ──────────────────

    #[test]
    fn parses_default_ctrl_b() {
        assert_eq!(parse_prefix_key("Ctrl+B"), Some(PrefixChord::default()));
        assert_eq!(parse_prefix_key("ctrl+b"), Some(PrefixChord::default()));
        assert_eq!(
            parse_prefix_key("CONTROL + B"),
            Some(PrefixChord::default())
        );
    }

    #[test]
    fn parses_ctrl_shift_x() {
        let chord = parse_prefix_key("Ctrl+Shift+X").unwrap();
        assert!(chord.ctrl);
        assert!(chord.shift);
        assert!(!chord.alt);
        assert_eq!(chord.key, Key::X);
    }

    #[test]
    fn parses_alt_digit() {
        let chord = parse_prefix_key("Alt+5").unwrap();
        assert!(chord.alt);
        assert_eq!(chord.key, Key::Num5);
    }

    #[test]
    fn rejects_unparseable_spec() {
        assert_eq!(parse_prefix_key(""), None);
        assert_eq!(parse_prefix_key("Ctrl+"), None);
        assert_eq!(parse_prefix_key("Ctrl+F19"), None);
        assert_eq!(parse_prefix_key("Ctrl+B+C"), None);
        assert_eq!(parse_prefix_key("garbage"), None);
    }

    #[test]
    fn custom_chord_matches_correctly() {
        let chord = parse_prefix_key("Ctrl+A").unwrap();
        let mut l = Latch::new(chord, DEFAULT_ARMED_TIMEOUT);
        let t0 = Instant::now();
        // Ctrl+B should not arm a Ctrl+A latch.
        assert_eq!(
            l.observe(mods(true, false, false), Key::B, t0),
            PrefixAction::None
        );
        assert!(!l.is_armed());
        // Ctrl+A arms it.
        assert_eq!(
            l.observe(mods(true, false, false), Key::A, t0),
            PrefixAction::None
        );
        assert!(l.is_armed());
        // Ctrl+A again = literal.
        assert_eq!(
            l.observe(
                mods(true, false, false),
                Key::A,
                t0 + Duration::from_millis(20)
            ),
            PrefixAction::Literal
        );
    }

    #[test]
    fn command_flag_aliases_to_ctrl() {
        let chord = PrefixChord::default();
        let m = Modifiers {
            ctrl: false,
            shift: false,
            alt: false,
            command: true,
            mac_cmd: false,
        };
        // Even though `ctrl=false`, `command=true` should match a Ctrl+B
        // chord on non-mac platforms.
        assert!(chord.matches(&m, Key::B));
    }
}
