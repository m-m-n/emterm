//! Prefix-key state machine for mux mode.
//!
//! Default chord: `Ctrl+Z`. After observing the prefix the latch is **armed**
//! and the next key drives one of:
//!
//! | Follow-up key        | Action                                    |
//! |----------------------|-------------------------------------------|
//! | `Ctrl+N`             | `PrefixAction::NextWindow`                |
//! | `Ctrl+P`             | `PrefixAction::PrevWindow`                |
//! | `Ctrl+D`             | `PrefixAction::Detach`                    |
//! | `0`..=`9`            | `PrefixAction::SelectWindow(<digit>)`     |
//! | the prefix chord     | `PrefixAction::Literal` (passes `0x1A`)   |
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
/// (`Ctrl+Z Ctrl+Z`). `0x1A` is the C0 control code for `Ctrl+Z`, which is
/// what a passthrough would have produced.
pub const DEFAULT_LITERAL_BYTE: u8 = 0x1A;

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
    /// to programs that themselves use `Ctrl+Z`.
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
    /// Toggle the mux window-list sidebar overlay (tmux-less; new for the
    /// mux-vertical-tabs feature). Only takes effect when
    /// `settings.mux.window_sidebar_overlay` is `true`; a strict no-op in
    /// persistent mode (FR4). Handled by `App::dispatch_mux_action`.
    ToggleWindowSidebar,
    /// Switch to the next window that has a reported (uncleared) agent
    /// status, in display order with wrap-around — skipping windows with no
    /// status and stopping at the current window when it is the only
    /// qualifying one (SPEC mux-agent-tab-cycle FR2/FR3/FR5/FR6). A no-op
    /// when zero windows qualify. Handled by `App::dispatch_mux_action`.
    NextAgentWindow,
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
    /// toggle-window-sidebar
    pub toggle_window_sidebar: PrefixChord,
    /// next-agent-window
    pub next_agent_window: PrefixChord,
}

/// `Ctrl`-modified single-letter [`PrefixChord`], used by the default
/// follow-up bindings in [`DEFAULT_ACTION_BINDINGS`].
const fn ctrl_letter(c: char) -> PrefixChord {
    PrefixChord {
        ctrl: true,
        shift: false,
        alt: false,
        key: KeySym::Letter(c),
    }
}

/// Single-source-of-truth table of the default follow-up chords per mux
/// action (all `Ctrl`-modified). `ActionBindings::default()` and
/// [`crate::settings::default_mux_action_chord`] both read from here so
/// the two can never drift. The WebView settings panel also reads these
/// (as chord strings) through the `get_mux_action_defaults` IPC command —
/// see [`default_action_bindings_as_strings`] — instead of keeping its own
/// copy, so there is no Rust↔TS default table to keep in sync.
pub const DEFAULT_ACTION_BINDINGS: &[(&str, PrefixChord)] = &[
    ("detach", ctrl_letter('d')),
    ("new-window", ctrl_letter('c')),
    ("next-window", ctrl_letter('n')),
    ("prev-window", ctrl_letter('p')),
    ("rename-window", ctrl_letter('r')),
    ("move-window", ctrl_letter('t')),
    ("toggle-window-sidebar", ctrl_letter('w')),
    ("next-agent-window", ctrl_letter('a')),
];

/// Default follow-up chord for a mux action, or `None` if the action
/// name is unknown. Lookup against [`DEFAULT_ACTION_BINDINGS`].
pub fn default_action_chord(action: &str) -> Option<PrefixChord> {
    DEFAULT_ACTION_BINDINGS
        .iter()
        .find_map(|(k, c)| (*k == action).then_some(*c))
}

/// Format a [`PrefixChord`] back into the `settings.mux` chord-string form
/// (e.g. `"Ctrl+T"`) — the inverse of [`parse_prefix_key`]. Modifiers are
/// emitted in `Ctrl`/`Shift`/`Alt` order and a `Letter` key is upper-cased
/// to match the WebView capture format, so the result round-trips through
/// `parse_prefix_key`.
///
/// [`KeySym::Other`] has no settings-string representation (it would yield a
/// keyless `"Ctrl+"` that `parse_prefix_key` rejects, breaking the round-trip).
/// No current caller can reach it — [`DEFAULT_ACTION_BINDINGS`] holds only
/// letter chords — so this is guarded with a `debug_assert!` rather than
/// changing the return type. Add an `Option` return if a real `Other` caller
/// ever appears.
pub fn format_chord(chord: &PrefixChord) -> String {
    debug_assert!(
        !matches!(chord.key, KeySym::Other),
        "format_chord: KeySym::Other has no settings-string form"
    );
    let mut parts: Vec<String> = Vec::new();
    if chord.ctrl {
        parts.push("Ctrl".to_string());
    }
    if chord.shift {
        parts.push("Shift".to_string());
    }
    if chord.alt {
        parts.push("Alt".to_string());
    }
    let key = match chord.key {
        KeySym::Letter(c) => c.to_ascii_uppercase().to_string(),
        KeySym::Digit(d) => ((b'0' + d) as char).to_string(),
        KeySym::Comma => ",".to_string(),
        KeySym::Period => ".".to_string(),
        KeySym::Semicolon => ";".to_string(),
        KeySym::Slash => "/".to_string(),
        KeySym::Backslash => "\\".to_string(),
        KeySym::Minus => "-".to_string(),
        KeySym::Other => String::new(),
    };
    parts.push(key);
    parts.join("+")
}

/// The default action bindings as `(action, chord_string)` pairs in the
/// `settings.mux.keybinds` string form, derived from
/// [`DEFAULT_ACTION_BINDINGS`] in declaration order. The settings panel
/// reads these via the `get_mux_action_defaults` IPC command so it never
/// duplicates the table in TypeScript.
pub fn default_action_bindings_as_strings() -> Vec<(&'static str, String)> {
    DEFAULT_ACTION_BINDINGS
        .iter()
        .map(|(action, chord)| (*action, format_chord(chord)))
        .collect()
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
            toggle_window_sidebar: get("toggle-window-sidebar"),
            next_agent_window: get("next-agent-window"),
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
            toggle_window_sidebar: map
                .get("toggle-window-sidebar")
                .copied()
                .unwrap_or(d.toggle_window_sidebar),
            next_agent_window: map
                .get("next-agent-window")
                .copied()
                .unwrap_or(d.next_agent_window),
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
        } else if self.toggle_window_sidebar.matches_as_follow_up(input) {
            Some(PrefixAction::ToggleWindowSidebar)
        } else if self.next_agent_window.matches_as_follow_up(input) {
            Some(PrefixAction::NextAgentWindow)
        } else {
            None
        }
    }
}

/// Configurable prefix chord. Constructed by parsing `settings.mux.prefix_key`
/// at startup (see `super::prefix::parse_prefix_key`). The default is
/// `Ctrl+Z`.
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
            key: KeySym::Letter('z'),
        }
    }
}

impl PrefixChord {
    /// Match the chord against a framework-agnostic [`KeyInput`] with
    /// exact modifier comparison. Used for the prefix chord itself
    /// (where the user expects, e.g., `Ctrl+Z` to require the Control
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
    /// `matchActionBinding` (`src-tauri/web-shared/terminal/mux/prefix-key.ts`): a bare
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
    /// hardcoded `Ctrl+Z` byte (`0x1A`). The classic case: a tmux nested
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

/// Parse a textual prefix-key spec like `"Ctrl+Z"` into a [`PrefixChord`].
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
mod tests;
