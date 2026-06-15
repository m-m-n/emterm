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
//! The module is intentionally pure — no I/O, no `egui::Context`, no logger,
//! no egui types. Input is fed in through the framework-agnostic
//! [`KeyInput`] / [`KeySym`] types defined below; the UI layer
//! (`window_host`) converts its native event source (egui, in this build)
//! into them before calling [`Latch::observe`] / [`crate::app::App::observe_mux_key`].

use std::time::{Duration, Instant};

/// Framework-agnostic key identity recognized by the mux prefix layer.
/// Carries only the discriminants the latch actually needs to match — the
/// letters, digits, and punctuation keys reachable by the tmux defaults
/// and the configurable `mux.keybinds`. Everything else (function keys,
/// arrows, navigation) becomes [`KeySym::Other`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeySym {
    /// An ASCII letter, normalized to lower case.
    Letter(char),
    /// A decimal digit `0..=9` (for `prefix N` window jumps).
    Digit(u8),
    Comma,
    Period,
    Semicolon,
    Slash,
    Backslash,
    Minus,
    /// Any key the latch does not recognize as a mux follow-up.
    Other,
}

impl KeySym {
    /// Match against the resolved follow-up character a `mux.keybinds`
    /// entry would use; returns the same lower-case `char` as the
    /// `default_mux_action_key` / `parse_mux_action_key` pipeline.
    fn as_char(self) -> Option<char> {
        match self {
            KeySym::Letter(c) => Some(c),
            KeySym::Comma => Some(','),
            KeySym::Period => Some('.'),
            KeySym::Semicolon => Some(';'),
            KeySym::Slash => Some('/'),
            KeySym::Backslash => Some('\\'),
            KeySym::Minus => Some('-'),
            _ => None,
        }
    }

    fn as_digit(self) -> Option<u8> {
        match self {
            KeySym::Digit(d) => Some(d),
            _ => None,
        }
    }
}

/// Modifier + key bundle the latch consumes. Plain data with no UI-toolkit
/// dependency: the window host converts its native event before calling
/// [`Latch::observe`]. `ctrl` MUST be set when either the actual Control
/// modifier OR egui's `command` (mac-Cmd / Windows-Ctrl alias) is held.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KeyInput {
    pub ctrl: bool,
    pub shift: bool,
    pub alt: bool,
    pub key: KeySym,
}

impl KeyInput {
    /// Convenience for tests: build a plain `ctrl+letter` chord.
    #[cfg(test)]
    pub fn ctrl_letter(letter: char) -> Self {
        Self {
            ctrl: true,
            shift: false,
            alt: false,
            key: KeySym::Letter(letter.to_ascii_lowercase()),
        }
    }
    /// Convenience for tests: a plain letter with no modifiers.
    #[cfg(test)]
    pub fn letter(letter: char) -> Self {
        Self {
            ctrl: false,
            shift: false,
            alt: false,
            key: KeySym::Letter(letter.to_ascii_lowercase()),
        }
    }
    /// Convenience for tests: a plain digit with no modifiers.
    #[cfg(test)]
    pub fn digit(d: u8) -> Self {
        Self {
            ctrl: false,
            shift: false,
            alt: false,
            key: KeySym::Digit(d),
        }
    }
}

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
    /// Create a new mux window (tmux `prefix c`). The caller sends
    /// `CreateWindow` and increments the pending-create count.
    NewWindow,
    /// Open the rename-window dialog (tmux `prefix ,`).
    RenameWindow,
    /// Open the move-window dialog (tmux `prefix m`).
    MoveWindow,
}

/// Effective follow-up key bindings for the mux actions, resolved from
/// `settings.mux.keybinds` (with tmux-compatible defaults). The latch
/// matches an armed follow-up [`KeyInput`] against each chord; digits
/// `0..=9` are always `SelectWindow` and are not configurable (matching
/// the WebView, where digit jumps are built in rather than part of
/// `keybinds`).
///
/// The chord representation lets a user bind, e.g., `detach = Ctrl+D`
/// instead of the bare `d` follow-up — matching the WebView's
/// `matchActionBinding` (which accepts both single-char and modifier
/// chords). Stored as [`PrefixChord`] so a single chord type covers both
/// the prefix and the action follow-ups.
#[derive(Debug, Clone)]
pub struct ActionBindings {
    /// detach
    pub detach: PrefixChord,
    /// new-window
    pub new_window: PrefixChord,
    /// next-window
    pub next_window: PrefixChord,
    /// prev-window
    pub prev_window: PrefixChord,
    /// rename-window
    pub rename_window: PrefixChord,
    /// move-window
    pub move_window: PrefixChord,
}

const fn bare_letter(c: char) -> PrefixChord {
    PrefixChord {
        ctrl: false,
        shift: false,
        alt: false,
        key: KeySym::Letter(c),
    }
}

const fn bare_comma() -> PrefixChord {
    PrefixChord {
        ctrl: false,
        shift: false,
        alt: false,
        key: KeySym::Comma,
    }
}

/// Single-source-of-truth table of the tmux-compatible default follow-up
/// chords per mux action. `ActionBindings::default()` and
/// [`crate::settings::default_mux_action_chord`] both read from here so
/// the two can never drift; mirror of the WebView `DEFAULT_ACTION_BINDINGS`
/// in `prefix-key.ts`.
pub const DEFAULT_ACTION_BINDINGS: &[(&str, PrefixChord)] = &[
    ("detach", bare_letter('d')),
    ("new-window", bare_letter('c')),
    ("next-window", bare_letter('n')),
    ("prev-window", bare_letter('p')),
    ("rename-window", bare_comma()),
    ("move-window", bare_letter('m')),
];

/// Default follow-up chord for a mux action, or `None` if the action
/// name is unknown. Lookup against [`DEFAULT_ACTION_BINDINGS`].
pub fn default_action_chord(action: &str) -> Option<PrefixChord> {
    DEFAULT_ACTION_BINDINGS
        .iter()
        .find_map(|(k, c)| (*k == action).then_some(*c))
}

impl Default for ActionBindings {
    fn default() -> Self {
        // Pull every field from the SSOT table — any drift surfaces at
        // construction time (the panic fires) instead of silently divergent
        // defaults.
        let get = |a| {
            default_action_chord(a).unwrap_or_else(|| panic!("DEFAULT_ACTION_BINDINGS missing {a}"))
        };
        Self {
            detach: get("detach"),
            new_window: get("new-window"),
            next_window: get("next-window"),
            prev_window: get("prev-window"),
            rename_window: get("rename-window"),
            move_window: get("move-window"),
        }
    }
}

impl ActionBindings {
    /// Build the table from a resolved `settings.mux.keybinds` map, falling
    /// back to the tmux default for any action the map omits. Invalid /
    /// unknown entries were already dropped (warn) by the settings loader,
    /// so this only reads validated chords.
    pub fn from_settings_map(map: &std::collections::HashMap<String, PrefixChord>) -> Self {
        let d = Self::default();
        Self {
            detach: map.get("detach").copied().unwrap_or(d.detach),
            new_window: map.get("new-window").copied().unwrap_or(d.new_window),
            next_window: map.get("next-window").copied().unwrap_or(d.next_window),
            prev_window: map.get("prev-window").copied().unwrap_or(d.prev_window),
            rename_window: map.get("rename-window").copied().unwrap_or(d.rename_window),
            move_window: map.get("move-window").copied().unwrap_or(d.move_window),
        }
    }

    /// Resolve an armed follow-up [`KeyInput`] to its action. Digits are
    /// mapped to `SelectWindow` before this is consulted; returns `None`
    /// for any input that does not match a bound chord (the caller then
    /// consumes the latch without emitting — unknown-after-prefix is
    /// ignored, FR2).
    pub fn action_for_input(&self, input: &KeyInput) -> Option<PrefixAction> {
        if self.detach.matches_as_follow_up(input) {
            Some(PrefixAction::Detach)
        } else if self.new_window.matches_as_follow_up(input) {
            Some(PrefixAction::NewWindow)
        } else if self.next_window.matches_as_follow_up(input) {
            Some(PrefixAction::NextWindow)
        } else if self.prev_window.matches_as_follow_up(input) {
            Some(PrefixAction::PrevWindow)
        } else if self.rename_window.matches_as_follow_up(input) {
            Some(PrefixAction::RenameWindow)
        } else if self.move_window.matches_as_follow_up(input) {
            Some(PrefixAction::MoveWindow)
        } else {
            None
        }
    }
}

/// Configurable prefix chord. Constructed by parsing `settings.mux.prefix_key`
/// at startup (see `super::prefix::parse_prefix_key`). The default is
/// `Ctrl+B`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PrefixChord {
    pub ctrl: bool,
    pub shift: bool,
    pub alt: bool,
    pub key: KeySym,
}

impl Default for PrefixChord {
    fn default() -> Self {
        Self {
            ctrl: true,
            shift: false,
            alt: false,
            key: KeySym::Letter('b'),
        }
    }
}

impl PrefixChord {
    /// Match the chord against a framework-agnostic [`KeyInput`] with
    /// exact modifier comparison. Used for the prefix chord itself
    /// (where the user expects, e.g., `Ctrl+B` to require the Control
    /// modifier).
    fn matches(&self, input: &KeyInput) -> bool {
        input.ctrl == self.ctrl
            && input.shift == self.shift
            && input.alt == self.alt
            && input.key == self.key
    }

    /// `true` when this chord carries no modifier (a bare single-key
    /// follow-up like `d` / `c` / `,`).
    fn is_bare(&self) -> bool {
        !self.ctrl && !self.shift && !self.alt
    }

    /// Match the chord as an **action follow-up**. Mirrors the WebView's
    /// `matchActionBinding` (`src/terminal/mux/prefix-key.ts`): a bare
    /// single-key binding (no modifiers) ignores the input's modifiers
    /// and matches the key alone, while a modifier-bearing binding
    /// requires exact modifier+key match. Without this, a user holding
    /// the prefix's Control modifier while typing the follow-up
    /// (`prefix → Ctrl+d`) would silently fail to dispatch `Detach`
    /// under the default `detach = "d"` binding even though the WebView
    /// fires it. Keeping `matches` for the prefix itself and
    /// `matches_as_follow_up` for action bindings preserves the strict
    /// semantics where they belong.
    fn matches_as_follow_up(&self, input: &KeyInput) -> bool {
        if self.is_bare() {
            input.key == self.key
        } else {
            self.matches(input)
        }
    }
}

/// Derive the byte to send on a double-prefix `Literal` from the active
/// `PrefixChord`. Used by `Latch::literal_byte` and tested independently;
/// see the doc on `Latch::literal_byte` for the resolution rules.
fn chord_literal_byte(chord: &PrefixChord) -> u8 {
    let ch = match chord.key {
        KeySym::Letter(c) => c,
        KeySym::Digit(d) => (b'0' + d) as char,
        KeySym::Comma => ',',
        KeySym::Period => '.',
        KeySym::Semicolon => ';',
        KeySym::Slash => '/',
        KeySym::Backslash => '\\',
        KeySym::Minus => '-',
        KeySym::Other => return DEFAULT_LITERAL_BYTE,
    };
    if chord.ctrl {
        // C0 control codes for `Ctrl+A` ... `Ctrl+Z` are 0x01..=0x1A; for
        // non-letter chord keys with Ctrl, fall back to the default literal
        // byte (the platform-level mapping of, say, "Ctrl+," varies).
        let upper = ch.to_ascii_uppercase();
        if upper.is_ascii_alphabetic() {
            return (upper as u8) - b'A' + 1;
        }
        DEFAULT_LITERAL_BYTE
    } else {
        // Bare printable chord (e.g. user set the prefix to a plain `,`): send
        // the printable byte directly. Non-ASCII chord keys never reach the
        // printable-byte path because they only arise via the keybinds parser,
        // which restricts to ASCII letters / digits / a few punctuation marks.
        ch as u32 as u8
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
    bindings: ActionBindings,
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
            bindings: ActionBindings::default(),
        }
    }

    /// Construct a latch with explicit action bindings (from
    /// `settings.mux.keybinds`).
    pub fn with_bindings(chord: PrefixChord, timeout: Duration, bindings: ActionBindings) -> Self {
        Self {
            chord,
            timeout,
            state: State::Idle,
            bindings,
        }
    }

    /// Replace the action bindings at runtime (settings apply). Does not
    /// disturb the arm state.
    pub fn set_bindings(&mut self, bindings: ActionBindings) {
        self.bindings = bindings;
    }

    /// Replace the prefix chord at runtime (settings apply). Cancels any
    /// in-flight arm so a stale chord can't fire under the new binding.
    pub fn set_chord(&mut self, chord: PrefixChord) {
        self.chord = chord;
        self.state = State::Idle;
    }

    /// Returns true when the latch is currently waiting for a follow-up key
    /// (used by the UI to draw a transient "prefix armed" indicator).
    pub fn is_armed(&self) -> bool {
        matches!(self.state, State::Armed { .. })
    }

    /// The byte to passthrough on a double-prefix ([`PrefixAction::Literal`]).
    /// Derived from the configured chord so a non-default prefix
    /// (e.g. `Ctrl+A`) sends its own C0 byte (`0x01`) instead of the
    /// hardcoded `Ctrl+B` byte (`0x02`). The classic case: a tmux nested
    /// inside the mux session expects to receive its own configured prefix
    /// when the user double-taps it.
    ///
    /// Resolution rules:
    /// - `ctrl + letter` → C0 control byte (`A`→0x01 … `Z`→0x1A).
    /// - non-ctrl printable chord → the printable byte itself.
    /// - anything else (Other / no representable byte) → fall back to
    ///   [`DEFAULT_LITERAL_BYTE`].
    pub fn literal_byte(&self) -> u8 {
        chord_literal_byte(&self.chord)
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
    pub fn observe(&mut self, input: &KeyInput, now: Instant) -> PrefixAction {
        // Auto-cancel if the previous arm has expired. Do this before
        // matching so a stale arm cannot accidentally trigger.
        if let State::Armed { since } = self.state {
            if now.duration_since(since) >= self.timeout {
                self.state = State::Idle;
            }
        }

        match self.state {
            State::Idle => {
                if self.chord.matches(input) {
                    self.state = State::Armed { since: now };
                }
                PrefixAction::None
            }
            State::Armed { .. } => {
                // Double-press: the same chord while armed is a literal
                // passthrough request.
                if self.chord.matches(input) {
                    self.state = State::Idle;
                    return PrefixAction::Literal;
                }
                let action = decode_follow_up(input, &self.bindings);
                self.state = State::Idle;
                action
            }
        }
    }
}

/// Decode the follow-up [`KeyInput`] into an action using the configured
/// [`ActionBindings`]. Digits `0..=9` always map to `SelectWindow` (built
/// in, not configurable, matching the WebView). Any other input is
/// matched against each bound chord (modifiers included); an unbound
/// input returns `PrefixAction::None` and the caller still consumes the
/// latch (the prefix chord is "used up" once armed — unknown-after-prefix
/// is ignored, FR2).
///
/// Digit recognition is restricted to bare digits (no modifiers) so a
/// modifier-bearing chord like `Ctrl+1` could be bound to a real action
/// without colliding with the built-in `SelectWindow(1)` jump. tmux
/// itself accepts only the bare digits for window selection, so this is
/// the natural match.
fn decode_follow_up(input: &KeyInput, bindings: &ActionBindings) -> PrefixAction {
    if !input.ctrl && !input.shift && !input.alt {
        if let Some(d) = input.key.as_digit() {
            return PrefixAction::SelectWindow(d);
        }
    }
    bindings
        .action_for_input(input)
        .unwrap_or(PrefixAction::None)
}

/// Parse a textual prefix-key spec like `"Ctrl+B"` into a [`PrefixChord`].
///
/// Recognized forms (case-insensitive on modifier names, case-insensitive on
/// single-letter keys):
///
/// - `Ctrl+B`, `Ctrl+Shift+B`, `Alt+X`, …
/// - Single-letter keys map to `KeySym::Letter('a'..='z')`.
/// - Digits `0..9` map to `KeySym::Digit(0..=9)`.
///
/// Returns `None` on an unparseable spec; the caller falls back to
/// [`PrefixChord::default`] and logs a `warn`.
pub fn parse_prefix_key(spec: &str) -> Option<PrefixChord> {
    let mut ctrl = false;
    let mut shift = false;
    let mut alt = false;
    let mut key: Option<KeySym> = None;
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
                key?; // bail if the token didn't map to any KeySym
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

fn parse_key_token(tok: &str) -> Option<KeySym> {
    if tok.len() == 1 {
        let c = tok.chars().next().unwrap();
        if c.is_ascii_alphabetic() {
            return Some(KeySym::Letter(c.to_ascii_lowercase()));
        }
        if let Some(digit) = c.to_digit(10) {
            return Some(KeySym::Digit(digit as u8));
        }
        // Single-character punctuation reachable from a `mux.keybinds`
        // chord spec. The set matches `KeySym::as_char` so a round-trip
        // ("," → KeySym::Comma → back to ",") is byte-identical.
        return match c {
            ',' => Some(KeySym::Comma),
            '.' => Some(KeySym::Period),
            ';' => Some(KeySym::Semicolon),
            '/' => Some(KeySym::Slash),
            '\\' => Some(KeySym::Backslash),
            '-' => Some(KeySym::Minus),
            _ => None,
        };
    }
    // Name-form token aliases — the WebView settings UI
    // (`src/settings/sections/mux-section.ts:256-262`) captures `,` as
    // the literal `,` *unless* the user presses something the UI rewrites
    // to a name, and `src/keybind/matcher.ts`'s `KEY_MAP` recognises
    // additional name forms (`Comma`, `Period`, `Slash`, …) that the
    // matcher accepts. We accept the subset that the existing `KeySym`
    // variants can already represent; name forms requiring new KeySym
    // variants (`Space`, `Plus`, `Enter`, named navigation keys) are
    // intentionally left for a follow-up KeySym extension (the user
    // explicitly deferred the broader rewrite).
    match tok.to_ascii_lowercase().as_str() {
        "comma" => Some(KeySym::Comma),
        "period" => Some(KeySym::Period),
        "semicolon" => Some(KeySym::Semicolon),
        "slash" => Some(KeySym::Slash),
        "backslash" => Some(KeySym::Backslash),
        "minus" => Some(KeySym::Minus),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper: the default Ctrl+B prefix as a `KeyInput`.
    fn prefix() -> KeyInput {
        KeyInput::ctrl_letter('b')
    }

    // ── TS-prefix-1: arming + follow-up actions ─────────────────────────

    #[test]
    fn ctrl_b_arms_the_latch_without_emitting_action() {
        let mut l = Latch::default();
        let now = Instant::now();
        assert_eq!(l.observe(&prefix(), now), PrefixAction::None);
        assert!(l.is_armed());
    }

    #[test]
    fn ctrl_b_then_n_emits_next_window() {
        let mut l = Latch::default();
        let t0 = Instant::now();
        let _ = l.observe(&prefix(), t0);
        let t1 = t0 + Duration::from_millis(50);
        assert_eq!(
            l.observe(&KeyInput::letter('n'), t1),
            PrefixAction::NextWindow
        );
        assert!(!l.is_armed());
    }

    #[test]
    fn ctrl_b_then_p_emits_prev_window() {
        let mut l = Latch::default();
        let t0 = Instant::now();
        let _ = l.observe(&prefix(), t0);
        assert_eq!(
            l.observe(&KeyInput::letter('p'), t0 + Duration::from_millis(50)),
            PrefixAction::PrevWindow
        );
    }

    #[test]
    fn ctrl_b_then_d_emits_detach() {
        let mut l = Latch::default();
        let t0 = Instant::now();
        let _ = l.observe(&prefix(), t0);
        assert_eq!(
            l.observe(&KeyInput::letter('d'), t0 + Duration::from_millis(50)),
            PrefixAction::Detach
        );
    }

    #[test]
    fn ctrl_b_then_digit_emits_select_window() {
        for want in [0u8, 1, 5, 9] {
            let mut l = Latch::default();
            let t0 = Instant::now();
            let _ = l.observe(&prefix(), t0);
            assert_eq!(
                l.observe(&KeyInput::digit(want), t0 + Duration::from_millis(50)),
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
        let _ = l.observe(&prefix(), t0);
        assert_eq!(
            l.observe(&prefix(), t0 + Duration::from_millis(20)),
            PrefixAction::Literal
        );
        assert!(!l.is_armed());
    }

    #[test]
    fn armed_latch_auto_cancels_after_3s() {
        let mut l = Latch::default();
        let t0 = Instant::now();
        let _ = l.observe(&prefix(), t0);
        assert!(l.is_armed());
        let later = t0 + DEFAULT_ARMED_TIMEOUT;
        assert_eq!(l.observe(&KeyInput::letter('n'), later), PrefixAction::None);
        assert!(!l.is_armed());
    }

    #[test]
    fn armed_latch_consumes_on_unknown_follow_up() {
        let mut l = Latch::default();
        let t0 = Instant::now();
        let _ = l.observe(&prefix(), t0);
        assert_eq!(
            l.observe(&KeyInput::letter('q'), t0 + Duration::from_millis(20)),
            PrefixAction::None
        );
        assert!(!l.is_armed());
    }

    #[test]
    fn cancel_clears_arm() {
        let mut l = Latch::default();
        let _ = l.observe(&prefix(), Instant::now());
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
        assert_eq!(chord.key, KeySym::Letter('x'));
    }

    #[test]
    fn parses_alt_digit() {
        let chord = parse_prefix_key("Alt+5").unwrap();
        assert!(chord.alt);
        assert_eq!(chord.key, KeySym::Digit(5));
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
            l.observe(&KeyInput::ctrl_letter('b'), t0),
            PrefixAction::None
        );
        assert!(!l.is_armed());
        // Ctrl+A arms it.
        assert_eq!(
            l.observe(&KeyInput::ctrl_letter('a'), t0),
            PrefixAction::None
        );
        assert!(l.is_armed());
        // Ctrl+A again = literal.
        assert_eq!(
            l.observe(&KeyInput::ctrl_letter('a'), t0 + Duration::from_millis(20)),
            PrefixAction::Literal
        );
    }

    // ── TS-11: extended actions + custom bindings ─────────────────────────

    fn arm_then(l: &mut Latch, follow: KeyInput) -> PrefixAction {
        let t0 = Instant::now();
        let _ = l.observe(&prefix(), t0);
        l.observe(&follow, t0 + Duration::from_millis(20))
    }

    #[test]
    fn default_bindings_map_c_comma_m() {
        let mut l = Latch::default();
        assert_eq!(
            arm_then(&mut l, KeyInput::letter('c')),
            PrefixAction::NewWindow
        );
        let mut l = Latch::default();
        let comma = KeyInput {
            ctrl: false,
            shift: false,
            alt: false,
            key: KeySym::Comma,
        };
        assert_eq!(arm_then(&mut l, comma), PrefixAction::RenameWindow);
        let mut l = Latch::default();
        assert_eq!(
            arm_then(&mut l, KeyInput::letter('m')),
            PrefixAction::MoveWindow
        );
    }

    #[test]
    fn default_bindings_still_map_d_n_p_and_digits() {
        let mut l = Latch::default();
        assert_eq!(
            arm_then(&mut l, KeyInput::letter('d')),
            PrefixAction::Detach
        );
        let mut l = Latch::default();
        assert_eq!(
            arm_then(&mut l, KeyInput::letter('n')),
            PrefixAction::NextWindow
        );
        let mut l = Latch::default();
        assert_eq!(
            arm_then(&mut l, KeyInput::letter('p')),
            PrefixAction::PrevWindow
        );
        let mut l = Latch::default();
        assert_eq!(
            arm_then(&mut l, KeyInput::digit(3)),
            PrefixAction::SelectWindow(3)
        );
    }

    #[test]
    fn unknown_follow_up_consumes_without_action() {
        let mut l = Latch::default();
        assert_eq!(arm_then(&mut l, KeyInput::letter('q')), PrefixAction::None);
        assert!(!l.is_armed());
    }

    #[test]
    fn custom_bindings_override_defaults() {
        let mut map = std::collections::HashMap::new();
        map.insert("next-window".to_string(), bare_letter('j'));
        map.insert("prev-window".to_string(), bare_letter('k'));
        let bindings = ActionBindings::from_settings_map(&map);
        let mut l = Latch::with_bindings(PrefixChord::default(), DEFAULT_ARMED_TIMEOUT, bindings);
        assert_eq!(
            arm_then(&mut l, KeyInput::letter('j')),
            PrefixAction::NextWindow
        );
        let mut l2 = {
            let mut map = std::collections::HashMap::new();
            map.insert("next-window".to_string(), bare_letter('j'));
            map.insert("prev-window".to_string(), bare_letter('k'));
            Latch::with_bindings(
                PrefixChord::default(),
                DEFAULT_ARMED_TIMEOUT,
                ActionBindings::from_settings_map(&map),
            )
        };
        assert_eq!(
            arm_then(&mut l2, KeyInput::letter('k')),
            PrefixAction::PrevWindow
        );
        let mut l3 = Latch::with_bindings(
            PrefixChord::default(),
            DEFAULT_ARMED_TIMEOUT,
            ActionBindings::from_settings_map(&{
                let mut m = std::collections::HashMap::new();
                m.insert("next-window".to_string(), bare_letter('j'));
                m
            }),
        );
        assert_eq!(arm_then(&mut l3, KeyInput::letter('n')), PrefixAction::None);
    }

    #[test]
    fn set_bindings_applies_at_runtime() {
        let mut l = Latch::default();
        let mut map = std::collections::HashMap::new();
        map.insert("new-window".to_string(), bare_letter('x'));
        l.set_bindings(ActionBindings::from_settings_map(&map));
        assert_eq!(
            arm_then(&mut l, KeyInput::letter('x')),
            PrefixAction::NewWindow
        );
        let mut l2 = l;
        assert_eq!(arm_then(&mut l2, KeyInput::letter('c')), PrefixAction::None);
    }

    #[test]
    fn double_prefix_still_literal_with_custom_bindings() {
        let bindings = ActionBindings::from_settings_map(&std::collections::HashMap::new());
        let mut l = Latch::with_bindings(PrefixChord::default(), DEFAULT_ARMED_TIMEOUT, bindings);
        let t0 = Instant::now();
        let _ = l.observe(&prefix(), t0);
        assert_eq!(
            l.observe(&prefix(), t0 + Duration::from_millis(20)),
            PrefixAction::Literal
        );
    }

    #[test]
    fn timeout_still_cancels_with_extended_actions() {
        let mut l = Latch::default();
        let t0 = Instant::now();
        let _ = l.observe(&prefix(), t0);
        let later = t0 + DEFAULT_ARMED_TIMEOUT;
        assert_eq!(l.observe(&KeyInput::letter('c'), later), PrefixAction::None);
        assert!(!l.is_armed());
    }

    #[test]
    fn action_bindings_from_settings_map_uses_defaults_for_missing() {
        let b = ActionBindings::from_settings_map(&std::collections::HashMap::new());
        assert_eq!(b.detach, bare_letter('d'));
        assert_eq!(b.new_window, bare_letter('c'));
        assert_eq!(b.move_window, bare_letter('m'));
    }

    /// Regression: a chord follow-up like `Ctrl+D` (the bind that
    /// triggered the multi-review session) must fire its action under
    /// the new chord-aware dispatcher. tmux ships `bind C-d detach-client`
    /// in many user configs, which `tmux_conf::converter` writes back as
    /// `settings.mux.keybinds.detach = "Ctrl+D"`.
    #[test]
    fn ctrl_letter_chord_follow_up_fires_action() {
        // Build the chord through the public spec parser to mirror the
        // settings loader's exact code path.
        let mut map = std::collections::HashMap::new();
        map.insert(
            "detach".to_string(),
            crate::settings::parse_mux_action_chord("Ctrl+D").unwrap(),
        );
        let bindings = ActionBindings::from_settings_map(&map);
        let mut l = Latch::with_bindings(PrefixChord::default(), DEFAULT_ARMED_TIMEOUT, bindings);
        // `Ctrl+D` after the prefix must fire `Detach`.
        assert_eq!(
            arm_then(&mut l, KeyInput::ctrl_letter('d')),
            PrefixAction::Detach
        );
        // Bare `d` no longer fires (the binding was rewritten).
        let mut l2 = Latch::with_bindings(
            PrefixChord::default(),
            DEFAULT_ARMED_TIMEOUT,
            ActionBindings::from_settings_map(&{
                let mut m = std::collections::HashMap::new();
                m.insert(
                    "detach".to_string(),
                    crate::settings::parse_mux_action_chord("Ctrl+D").unwrap(),
                );
                m
            }),
        );
        assert_eq!(arm_then(&mut l2, KeyInput::letter('d')), PrefixAction::None);
    }

    /// A bare digit must still trigger `SelectWindow`, but a modifier
    /// chord on a digit (`Ctrl+1`) must NOT — it falls through to the
    /// action bindings so a user could in principle bind it.
    #[test]
    fn modifier_digit_chord_does_not_trigger_select_window() {
        let mut l = Latch::default();
        // Bare 1 → SelectWindow(1).
        assert_eq!(
            arm_then(&mut l, KeyInput::digit(1)),
            PrefixAction::SelectWindow(1)
        );
        // Ctrl+1 → falls through to bindings (no default binding) → None.
        let mut l2 = Latch::default();
        assert_eq!(
            arm_then(
                &mut l2,
                KeyInput {
                    ctrl: true,
                    shift: false,
                    alt: false,
                    key: KeySym::Digit(1),
                }
            ),
            PrefixAction::None
        );
    }

    /// SPEC regression — mirrors WebView's `matchActionBinding` which
    /// matches a bare single-key binding (`detach = "d"`) against the
    /// input's KEY only, ignoring modifiers. Without this rule a user
    /// holding the prefix's Control modifier while typing the follow-up
    /// (`Ctrl+Z` prefix → still-holding-Ctrl `d`) would silently fail to
    /// detach under the default binding, even though the WebView fires
    /// it.
    #[test]
    fn bare_binding_ignores_modifiers_like_webview() {
        let mut l = Latch::default();
        // `Ctrl+d` after the prefix fires `Detach` because `detach = "d"`
        // is a bare binding (no modifiers in the binding spec).
        assert_eq!(
            arm_then(&mut l, KeyInput::ctrl_letter('d')),
            PrefixAction::Detach
        );
        // Same for `Shift+d`.
        let mut l2 = Latch::default();
        assert_eq!(
            arm_then(
                &mut l2,
                KeyInput {
                    ctrl: false,
                    shift: true,
                    alt: false,
                    key: KeySym::Letter('d'),
                }
            ),
            PrefixAction::Detach
        );
        // And the plain bare `d` still fires.
        let mut l3 = Latch::default();
        assert_eq!(
            arm_then(&mut l3, KeyInput::letter('d')),
            PrefixAction::Detach
        );
    }

    /// Modifier-bearing bindings (`detach = "Ctrl+D"`) keep strict
    /// modifier matching — bare `d` no longer fires under that binding.
    #[test]
    fn modifier_binding_keeps_strict_match() {
        let mut map = std::collections::HashMap::new();
        map.insert(
            "detach".to_string(),
            crate::settings::parse_mux_action_chord("Ctrl+D").unwrap(),
        );
        let bindings = ActionBindings::from_settings_map(&map);
        let mut l = Latch::with_bindings(PrefixChord::default(), DEFAULT_ARMED_TIMEOUT, bindings);
        // Bare `d` must NOT fire under a `Ctrl+D`-only binding.
        assert_eq!(arm_then(&mut l, KeyInput::letter('d')), PrefixAction::None);
    }

    /// SPEC regression — WebView settings UI captures keys with
    /// name-form aliases (`Comma`, `Period`, `Slash`, …). The native
    /// loader's chord-spec parser must accept those so a UI-saved
    /// `mux.keybinds.rename-window = "Ctrl+Comma"` is honoured instead
    /// of silently falling back to the default. Only the subset
    /// representable by the existing `KeySym` variants is covered here;
    /// `Space` / `Plus` / `Enter` etc. are intentionally deferred.
    #[test]
    fn name_form_punctuation_aliases_parse() {
        // Single-token name forms (no modifiers).
        let comma = crate::settings::parse_mux_action_chord("Comma").unwrap();
        let period = crate::settings::parse_mux_action_chord("Period").unwrap();
        let slash = crate::settings::parse_mux_action_chord("Slash").unwrap();
        let backslash = crate::settings::parse_mux_action_chord("Backslash").unwrap();
        let minus = crate::settings::parse_mux_action_chord("Minus").unwrap();
        let semicolon = crate::settings::parse_mux_action_chord("Semicolon").unwrap();
        assert_eq!(comma.key, KeySym::Comma);
        assert_eq!(period.key, KeySym::Period);
        assert_eq!(slash.key, KeySym::Slash);
        assert_eq!(backslash.key, KeySym::Backslash);
        assert_eq!(minus.key, KeySym::Minus);
        assert_eq!(semicolon.key, KeySym::Semicolon);
        // Case-insensitive.
        let lc = crate::settings::parse_mux_action_chord("comma").unwrap();
        assert_eq!(lc.key, KeySym::Comma);
        // Modifier + name form (matches the WebView UI's `Ctrl+Comma` save shape).
        let ctrl_comma = crate::settings::parse_mux_action_chord("Ctrl+Comma").unwrap();
        assert!(ctrl_comma.ctrl);
        assert_eq!(ctrl_comma.key, KeySym::Comma);
    }
}
