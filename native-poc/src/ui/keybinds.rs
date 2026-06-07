//! Central keybind dispatch.
//!
//! Phase 4-B implemented a pure mapping from `(egui::Modifiers,
//! egui::Key)` to [`AppAction`] with a hard-coded chord table. This
//! module now drives the tab-roster + view chords from `settings.json`'s
//! `keybinds.*` block via a [`KeybindTable`]: `new_tab`, `close_tab`,
//! `next_tab`, `prev_tab`, `select_all`, `search`,
//! `jump_to_prev_prompt`, `jump_to_next_prompt`, the zoom trio,
//! `toggle_fullscreen`, and `toggle_tab_bar` are resolved from
//! user-configured chord specs at startup. (`copy` / `paste` live one
//! layer up in `window_host::handle_special_chord`.)
//!
//! The dispatcher stays state-free apart from the borrowed
//! [`KeybindTable`], so it can be exercised directly from unit tests by
//! constructing synthetic `(table, mods, key)` triples — no egui
//! context, no tabs vector, no PTY required. The resulting `AppAction`
//! is then applied either through `App::apply_action`
//! (`NewTab` / `CloseTab` / `NextTab` / `PrevTab` / `JumpTab` /
//! `SelectAll`) or at the `window_host` layer for actions that need the
//! `winit::window::Window` or host resize machinery
//! (`ToggleFullscreen` / `ToggleTabBar` / the zoom trio).
//!
//! Settings-driven bindings (default specs shown):
//!
//! | Action              | Default spec    | `AppAction`        |
//! |---------------------|-----------------|--------------------|
//! | `new_tab`           | `Ctrl+Shift+T`  | `NewTab`           |
//! | `close_tab`         | `Ctrl+Shift+W`  | `CloseTab`         |
//! | `next_tab`          | `Ctrl+PageDown` | `NextTab`          |
//! | `prev_tab`          | `Ctrl+PageUp`   | `PrevTab`          |
//! | `select_all`        | `Ctrl+Shift+A`  | `SelectAll`        |
//! | `search`            | `Ctrl+Shift+F`  | `OpenSearch`       |
//! | `jump_to_prev_prompt` | `Ctrl+Shift+ArrowUp`   | `JumpToPrevPrompt` |
//! | `jump_to_next_prompt` | `Ctrl+Shift+ArrowDown` | `JumpToNextPrompt` |
//! | `zoom_in`           | `Ctrl+Plus`     | `ZoomIn`           |
//! | `zoom_out`          | `Ctrl+Minus`    | `ZoomOut`          |
//! | `zoom_reset`        | `Ctrl+0`        | `ZoomReset`        |
//! | `toggle_fullscreen` | `F11`           | `ToggleFullscreen` |
//! | `toggle_tab_bar`    | `Ctrl+Shift+B`  | `ToggleTabBar`     |
//!
//! Built-in bindings (native-poc conventions, not in `settings.json`,
//! never `alt`):
//!
//! | Chord            | Action                |
//! |------------------|-----------------------|
//! | Ctrl+Tab         | `NextTab`             |
//! | Ctrl+Shift+Tab   | `PrevTab`             |
//! | Ctrl+1 .. Ctrl+9 | `JumpTab(n)` (1-based)|
//!
//! Every other chord returns `None` and the caller should fall through
//! to the active PTY writer (or the clipboard / scrollback chord layer
//! in `window_host.rs`).

use egui::{Key, Modifiers};

use super::AppAction;
use crate::settings::KeybindSettings;

/// A parsed keyboard chord: a main [`egui::Key`] plus the modifier flags
/// that must accompany it. `command` / `mac_cmd` are intentionally
/// absent because native-poc targets Linux + Windows only; the dispatch
/// layer folds egui's `command` alias into `ctrl` before matching.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Chord {
    pub ctrl: bool,
    pub shift: bool,
    pub alt: bool,
    pub key: Key,
}

/// Parse a textual chord spec (e.g. `"Ctrl+Shift+C"`) into a [`Chord`].
///
/// Parsing rules (compatible with the frontend `src/keybind/matcher.ts`
/// `KEY_MAP`):
/// - Parts are split on `'+'` and trimmed; comparison is case-insensitive.
/// - Modifier tokens: `ctrl` / `control` → ctrl, `shift` → shift,
///   `alt` → alt. `meta` / `cmd` / `command` are not supported on
///   native-poc's targets and cause the whole spec to return `None`.
/// - Exactly one non-modifier "main key" token is required; zero or two
///   or more → `None`.
/// - An unrecognized main-key name → `None`.
pub fn parse_chord(spec: &str) -> Option<Chord> {
    let mut ctrl = false;
    let mut shift = false;
    let mut alt = false;
    let mut key: Option<Key> = None;

    for part in spec.split('+') {
        let token = part.trim();
        if token.is_empty() {
            // A stray empty token (e.g. trailing '+' or "Ctrl++") is not
            // a valid modifier nor a main key.
            return None;
        }
        let lower = token.to_ascii_lowercase();
        match lower.as_str() {
            "ctrl" | "control" => ctrl = true,
            "shift" => shift = true,
            "alt" => alt = true,
            // Platform modifiers native-poc does not honor. Reject the
            // whole spec rather than silently dropping the modifier so a
            // mac-targeted binding never collapses onto a bare key.
            "meta" | "cmd" | "command" => return None,
            _ => {
                // A main key. A second main key is ambiguous → reject.
                if key.is_some() {
                    return None;
                }
                match parse_main_key(&lower) {
                    Some(k) => key = Some(k),
                    None => return None,
                }
            }
        }
    }

    Some(Chord {
        ctrl,
        shift,
        alt,
        key: key?,
    })
}

/// Map a normalized (already lowercased, trimmed) main-key token to an
/// [`egui::Key`]. Returns `None` for tokens that egui 0.29 cannot
/// represent. Mirrors the frontend `KEY_MAP` where the variant exists.
fn parse_main_key(lower: &str) -> Option<Key> {
    // Single ASCII letter / digit.
    if lower.len() == 1 {
        let c = lower.as_bytes()[0];
        if c.is_ascii_alphabetic() {
            // `Key::from_name` keys off the uppercase letter name, e.g.
            // "A". Build that single-char string from the byte (a plain
            // `u8::to_string()` would yield the decimal code point).
            return Key::from_name(&(c.to_ascii_uppercase() as char).to_string());
        }
        if c.is_ascii_digit() {
            return digit_key(c - b'0');
        }
    }

    // Function keys f1..f20 (egui 0.29 carries F1..F35; we expose the
    // common range the frontend KEY_MAP uses).
    if let Some(rest) = lower.strip_prefix('f') {
        if let Ok(n) = rest.parse::<u8>() {
            return fn_key(n);
        }
    }

    Some(match lower {
        "plus" | "+" => Key::Plus,
        "minus" | "-" => Key::Minus,
        "comma" | "," => Key::Comma,
        "period" | "." => Key::Period,
        "slash" | "/" => Key::Slash,
        "backslash" | "\\" => Key::Backslash,
        "space" => Key::Space,
        "enter" => Key::Enter,
        "escape" => Key::Escape,
        "tab" => Key::Tab,
        "backspace" => Key::Backspace,
        "delete" => Key::Delete,
        "insert" => Key::Insert,
        "arrowup" => Key::ArrowUp,
        "arrowdown" => Key::ArrowDown,
        "arrowleft" => Key::ArrowLeft,
        "arrowright" => Key::ArrowRight,
        "home" => Key::Home,
        "end" => Key::End,
        "pageup" => Key::PageUp,
        "pagedown" => Key::PageDown,
        "equals" | "=" => Key::Equals,
        "semicolon" | ";" => Key::Semicolon,
        "colon" | ":" => Key::Colon,
        _ => return None,
    })
}

/// Map `0..=9` to `Key::Num0..=Num9`.
fn digit_key(d: u8) -> Option<Key> {
    Some(match d {
        0 => Key::Num0,
        1 => Key::Num1,
        2 => Key::Num2,
        3 => Key::Num3,
        4 => Key::Num4,
        5 => Key::Num5,
        6 => Key::Num6,
        7 => Key::Num7,
        8 => Key::Num8,
        9 => Key::Num9,
        _ => return None,
    })
}

/// Map `1..=20` to `Key::F1..=F20`. egui 0.29 carries F1..F35; the
/// frontend KEY_MAP only references up to F20, so we cap there.
fn fn_key(n: u8) -> Option<Key> {
    Some(match n {
        1 => Key::F1,
        2 => Key::F2,
        3 => Key::F3,
        4 => Key::F4,
        5 => Key::F5,
        6 => Key::F6,
        7 => Key::F7,
        8 => Key::F8,
        9 => Key::F9,
        10 => Key::F10,
        11 => Key::F11,
        12 => Key::F12,
        13 => Key::F13,
        14 => Key::F14,
        15 => Key::F15,
        16 => Key::F16,
        17 => Key::F17,
        18 => Key::F18,
        19 => Key::F19,
        20 => Key::F20,
        _ => return None,
    })
}

/// The resolved chords native-poc dispatches today. `copy` / `paste`
/// are consumed in `window_host::handle_special_chord`; the tab-roster
/// chords are matched in [`dispatch`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KeybindTable {
    pub copy: Chord,
    pub paste: Chord,
    pub new_tab: Chord,
    pub close_tab: Chord,
    pub next_tab: Chord,
    pub prev_tab: Chord,
    pub select_all: Chord,
    pub search: Chord,
    pub jump_to_prev_prompt: Chord,
    pub jump_to_next_prompt: Chord,
    pub zoom_in: Chord,
    pub zoom_out: Chord,
    pub zoom_reset: Chord,
    pub toggle_fullscreen: Chord,
    pub toggle_tab_bar: Chord,
}

impl KeybindTable {
    /// Build a [`KeybindTable`] from the user's [`KeybindSettings`].
    /// Each action's spec is parsed by [`parse_chord`]; an unparseable
    /// spec falls back to the built-in default (which is guaranteed to
    /// parse) with a `warn`-level log so a typo in `settings.json` is
    /// visible instead of silently disabling the binding.
    pub fn from_settings(kb: &KeybindSettings) -> Self {
        let table = Self {
            copy: resolve("copy", &kb.copy),
            paste: resolve("paste", &kb.paste),
            new_tab: resolve("new_tab", &kb.new_tab),
            close_tab: resolve("close_tab", &kb.close_tab),
            next_tab: resolve("next_tab", &kb.next_tab),
            prev_tab: resolve("prev_tab", &kb.prev_tab),
            select_all: resolve("select_all", &kb.select_all),
            search: resolve("search", &kb.search),
            jump_to_prev_prompt: resolve("jump_to_prev_prompt", &kb.jump_to_prev_prompt),
            jump_to_next_prompt: resolve("jump_to_next_prompt", &kb.jump_to_next_prompt),
            zoom_in: resolve("zoom_in", &kb.zoom_in),
            zoom_out: resolve("zoom_out", &kb.zoom_out),
            zoom_reset: resolve("zoom_reset", &kb.zoom_reset),
            toggle_fullscreen: resolve("toggle_fullscreen", &kb.toggle_fullscreen),
            toggle_tab_bar: resolve("toggle_tab_bar", &kb.toggle_tab_bar),
        };
        // Matching is first-wins (`handle_special_chord` checks copy /
        // paste before `dispatch` walks the tab actions), so two actions
        // sharing one chord silently disable the lower-priority one.
        // Surface the collision the same way unparseable specs are
        // surfaced: a warn-level log naming both actions.
        for (winner, loser) in table.collisions() {
            log::warn!(
                "settings.keybinds: {} and {} share the same chord; only {} will fire",
                winner,
                loser,
                winner
            );
        }
        table
    }

    /// Pairs of actions whose resolved chords collide, as
    /// `(winner, loser)` in runtime match priority (copy and paste are
    /// checked in `handle_special_chord` before [`dispatch`] walks
    /// new_tab → close_tab → next_tab → prev_tab → select_all →
    /// zoom_in → zoom_out → zoom_reset → toggle_fullscreen →
    /// toggle_tab_bar). Pure so tests can assert collision detection
    /// without capturing logs.
    fn collisions(&self) -> Vec<(&'static str, &'static str)> {
        let entries = [
            ("copy", self.copy),
            ("paste", self.paste),
            ("new_tab", self.new_tab),
            ("close_tab", self.close_tab),
            ("next_tab", self.next_tab),
            ("prev_tab", self.prev_tab),
            ("select_all", self.select_all),
            ("search", self.search),
            ("jump_to_prev_prompt", self.jump_to_prev_prompt),
            ("jump_to_next_prompt", self.jump_to_next_prompt),
            ("zoom_in", self.zoom_in),
            ("zoom_out", self.zoom_out),
            ("zoom_reset", self.zoom_reset),
            ("toggle_fullscreen", self.toggle_fullscreen),
            ("toggle_tab_bar", self.toggle_tab_bar),
        ];
        let mut out = Vec::new();
        for i in 0..entries.len() {
            for j in (i + 1)..entries.len() {
                if entries[i].1 == entries[j].1 {
                    out.push((entries[i].0, entries[j].0));
                }
            }
        }
        out
    }
}

impl Default for KeybindTable {
    fn default() -> Self {
        Self::from_settings(&KeybindSettings::default())
    }
}

/// Parse `spec` for keybind `action`; on failure, log a warning and fall
/// back to the built-in default chord for that action. The defaults all
/// originate from [`KeybindSettings::default`] and are known to parse,
/// so the fallback `parse_chord(...).expect(...)` cannot panic.
fn resolve(action: &str, spec: &str) -> Chord {
    match parse_chord(spec) {
        Some(c) => c,
        None => {
            let default_spec = default_spec_for(action);
            log::warn!(
                "settings.keybinds.{}: unparseable {:?}, falling back to {:?}",
                action,
                spec,
                default_spec
            );
            parse_chord(default_spec).expect("built-in default keybind spec must parse")
        }
    }
}

/// The built-in default spec for an action name. Reads from a fresh
/// [`KeybindSettings::default`] so the fallback string always matches
/// the documented default.
fn default_spec_for(action: &str) -> &'static str {
    // Hard-coded mirror of `KeybindSettings::default()` for the actions
    // this table dispatches. Kept as `&'static str` so the warn-log can
    // reuse it and `resolve` can re-parse it without an allocation.
    match action {
        "copy" => "Ctrl+Shift+C",
        "paste" => "Ctrl+Shift+V",
        "new_tab" => "Ctrl+Shift+T",
        "close_tab" => "Ctrl+Shift+W",
        "next_tab" => "Ctrl+PageDown",
        "prev_tab" => "Ctrl+PageUp",
        "select_all" => "Ctrl+Shift+A",
        "search" => "Ctrl+Shift+F",
        "jump_to_prev_prompt" => "Ctrl+Shift+ArrowUp",
        "jump_to_next_prompt" => "Ctrl+Shift+ArrowDown",
        "zoom_in" => "Ctrl+Plus",
        "zoom_out" => "Ctrl+Minus",
        "zoom_reset" => "Ctrl+0",
        "toggle_fullscreen" => "F11",
        "toggle_tab_bar" => "Ctrl+Shift+B",
        // Unreachable: `resolve` is only called with the names above.
        _ => "Ctrl+Shift+T",
    }
}

/// Map an `(egui::Modifiers, egui::Key)` pair to an [`AppAction`] using
/// the resolved [`KeybindTable`].
///
/// Returns `None` for any chord that is not bound; the caller forwards
/// such inputs to the active PTY.
///
/// Matching order:
/// 1. The settings-driven tab-roster chords (`new_tab`, `close_tab`,
///    `next_tab`, `prev_tab`). These may include `alt`.
/// 2. The built-in conventions (`Ctrl+Tab`, `Ctrl+Shift+Tab`,
///    `Ctrl+1..9`), which are never `alt`.
///
/// The table considers the **logical** modifier flags (`ctrl`, `shift`,
/// `alt`). The egui `command` alias folds into `ctrl`; `mac_cmd` is
/// ignored because Phase 4 targets Linux + Windows only.
pub fn dispatch(table: &KeybindTable, mods: Modifiers, key: Key) -> Option<AppAction> {
    let ctrl = mods.ctrl || mods.command;
    let shift = mods.shift;
    let alt = mods.alt;

    let chord = Chord {
        ctrl,
        shift,
        alt,
        key,
    };

    // 1. Settings-driven chords take priority. Configured specs may
    //    include `alt`, so this check runs before the `!alt` built-ins.
    if chord == table.new_tab {
        return Some(AppAction::NewTab);
    }
    if chord == table.close_tab {
        return Some(AppAction::CloseTab);
    }
    if chord == table.next_tab {
        return Some(AppAction::NextTab);
    }
    if chord == table.prev_tab {
        return Some(AppAction::PrevTab);
    }
    if chord == table.select_all {
        return Some(AppAction::SelectAll);
    }
    if chord == table.search {
        return Some(AppAction::OpenSearch);
    }
    if chord == table.jump_to_prev_prompt {
        return Some(AppAction::JumpToPrevPrompt);
    }
    if chord == table.jump_to_next_prompt {
        return Some(AppAction::JumpToNextPrompt);
    }
    if chord == table.zoom_in {
        return Some(AppAction::ZoomIn);
    }
    if chord == table.zoom_out {
        return Some(AppAction::ZoomOut);
    }
    if chord == table.zoom_reset {
        return Some(AppAction::ZoomReset);
    }
    if chord == table.toggle_fullscreen {
        return Some(AppAction::ToggleFullscreen);
    }
    if chord == table.toggle_tab_bar {
        return Some(AppAction::ToggleTabBar);
    }

    // 2. Built-in native-poc conventions (never alt). These are not
    //    exposed in settings.json; they coexist with whatever the user
    //    configured for next/prev tab above.
    if alt {
        return None;
    }
    match (ctrl, shift, key) {
        // Cycling
        (true, false, Key::Tab) => Some(AppAction::NextTab),
        (true, true, Key::Tab) => Some(AppAction::PrevTab),

        // Direct jumps Ctrl+1 .. Ctrl+9. Shift is rejected so e.g.
        // Ctrl+Shift+1 stays available for the active shell.
        (true, false, Key::Num1) => Some(AppAction::JumpTab(1)),
        (true, false, Key::Num2) => Some(AppAction::JumpTab(2)),
        (true, false, Key::Num3) => Some(AppAction::JumpTab(3)),
        (true, false, Key::Num4) => Some(AppAction::JumpTab(4)),
        (true, false, Key::Num5) => Some(AppAction::JumpTab(5)),
        (true, false, Key::Num6) => Some(AppAction::JumpTab(6)),
        (true, false, Key::Num7) => Some(AppAction::JumpTab(7)),
        (true, false, Key::Num8) => Some(AppAction::JumpTab(8)),
        (true, false, Key::Num9) => Some(AppAction::JumpTab(9)),

        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper: build a `Modifiers` value with only the requested bits
    /// set. The egui `Modifiers::default()` is all-false on the fields
    /// we care about (ctrl/shift/alt/command/mac_cmd).
    fn mods(ctrl: bool, shift: bool, alt: bool) -> Modifiers {
        Modifiers {
            ctrl,
            shift,
            alt,
            command: false,
            mac_cmd: false,
        }
    }

    // TS-kb-1: keybind dispatch table — drive synthetic (mods, key)
    // pairs through `dispatch` against the default table and assert
    // AppAction equality. The default table is the Phase 4-B baseline so
    // these confirm the existing behavior is preserved.

    #[test]
    fn ctrl_shift_t_is_new_tab() {
        assert_eq!(
            dispatch(&KeybindTable::default(), mods(true, true, false), Key::T),
            Some(AppAction::NewTab)
        );
    }

    #[test]
    fn ctrl_shift_w_is_close_tab() {
        assert_eq!(
            dispatch(&KeybindTable::default(), mods(true, true, false), Key::W),
            Some(AppAction::CloseTab)
        );
    }

    #[test]
    fn ctrl_tab_is_next_tab() {
        assert_eq!(
            dispatch(&KeybindTable::default(), mods(true, false, false), Key::Tab),
            Some(AppAction::NextTab)
        );
    }

    #[test]
    fn ctrl_shift_tab_is_prev_tab() {
        assert_eq!(
            dispatch(&KeybindTable::default(), mods(true, true, false), Key::Tab),
            Some(AppAction::PrevTab)
        );
    }

    #[test]
    fn ctrl_digit_jumps_to_tab() {
        let table = KeybindTable::default();
        for (key, want) in [
            (Key::Num1, 1u8),
            (Key::Num2, 2),
            (Key::Num3, 3),
            (Key::Num4, 4),
            (Key::Num5, 5),
            (Key::Num6, 6),
            (Key::Num7, 7),
            (Key::Num8, 8),
            (Key::Num9, 9),
        ] {
            assert_eq!(
                dispatch(&table, mods(true, false, false), key),
                Some(AppAction::JumpTab(want)),
                "Ctrl+{want} should jump to tab {want}"
            );
        }
    }

    #[test]
    fn ctrl_zero_is_not_a_jump() {
        // The built-in jump table binds 1..=9 only; Ctrl+0 is never a
        // JumpTab. It now resolves to the settings-driven `zoom_reset`
        // chord (default `Ctrl+0`) rather than falling through to the
        // PTY — assert it is specifically not a JumpTab here, and let
        // `default_ctrl_zero_is_zoom_reset` cover the positive mapping.
        assert_ne!(
            dispatch(
                &KeybindTable::default(),
                mods(true, false, false),
                Key::Num0
            ),
            Some(AppAction::JumpTab(0))
        );
        assert_eq!(
            dispatch(
                &KeybindTable::default(),
                mods(true, false, false),
                Key::Num0
            ),
            Some(AppAction::ZoomReset)
        );
    }

    #[test]
    fn ctrl_shift_digit_does_not_jump() {
        // Ctrl+Shift+1 must NOT trigger JumpTab(1); apps that bind
        // Ctrl+Shift+digit (e.g. tmux profiles) need passthrough.
        assert_eq!(
            dispatch(&KeybindTable::default(), mods(true, true, false), Key::Num1),
            None
        );
    }

    #[test]
    fn alt_prefixed_chord_falls_through() {
        // Alt+Tab is window-manager territory; Alt+Shift+T must not
        // hijack the global keybind path either (default table has no
        // alt-bearing chords).
        let table = KeybindTable::default();
        assert_eq!(dispatch(&table, mods(false, false, true), Key::Tab), None);
        assert_eq!(dispatch(&table, mods(true, true, true), Key::T), None);
    }

    #[test]
    fn unbound_chord_returns_none() {
        // Plain "T", Ctrl+T (no Shift), Shift+T — all PTY-bound.
        let table = KeybindTable::default();
        assert_eq!(dispatch(&table, mods(false, false, false), Key::T), None);
        assert_eq!(dispatch(&table, mods(true, false, false), Key::T), None);
        assert_eq!(dispatch(&table, mods(false, true, false), Key::T), None);
    }

    #[test]
    fn command_alias_maps_to_ctrl() {
        // egui's `command` flag aliases to ctrl on non-mac. Make sure
        // a synthesized Cmd+Shift+T still routes to NewTab so any
        // platform abstraction layer above us is robust.
        let m = Modifiers {
            ctrl: false,
            shift: true,
            alt: false,
            command: true,
            mac_cmd: false,
        };
        assert_eq!(
            dispatch(&KeybindTable::default(), m, Key::T),
            Some(AppAction::NewTab)
        );
    }

    // ── settings-driven next/prev tab defaults ─────────────────────────

    #[test]
    fn default_ctrl_pagedown_is_next_tab() {
        assert_eq!(
            dispatch(
                &KeybindTable::default(),
                mods(true, false, false),
                Key::PageDown
            ),
            Some(AppAction::NextTab)
        );
    }

    #[test]
    fn default_ctrl_pageup_is_prev_tab() {
        assert_eq!(
            dispatch(
                &KeybindTable::default(),
                mods(true, false, false),
                Key::PageUp
            ),
            Some(AppAction::PrevTab)
        );
    }

    // ── parse_chord ────────────────────────────────────────────────────

    #[test]
    fn parse_chord_all_default_specs_parse() {
        let d = KeybindSettings::default();
        for spec in [
            &d.copy,
            &d.paste,
            &d.select_all,
            &d.search,
            &d.new_tab,
            &d.new_tab_global,
            &d.close_tab,
            &d.next_tab,
            &d.prev_tab,
            &d.zoom_in,
            &d.zoom_out,
            &d.zoom_reset,
            &d.toggle_fullscreen,
            &d.open_settings,
            &d.toggle_tab_bar,
            &d.jump_to_prev_prompt,
            &d.jump_to_next_prompt,
            &d.profile_selector,
        ] {
            assert!(
                parse_chord(spec).is_some(),
                "default spec {spec:?} must parse"
            );
        }
    }

    #[test]
    fn parse_chord_is_case_insensitive() {
        assert_eq!(
            parse_chord("ctrl+shift+c"),
            Some(Chord {
                ctrl: true,
                shift: true,
                alt: false,
                key: Key::C,
            })
        );
    }

    #[test]
    fn parse_chord_allows_surrounding_whitespace() {
        assert_eq!(
            parse_chord(" Ctrl + Shift + C "),
            Some(Chord {
                ctrl: true,
                shift: true,
                alt: false,
                key: Key::C,
            })
        );
    }

    #[test]
    fn parse_chord_rejects_invalid_specs() {
        assert_eq!(parse_chord(""), None);
        assert_eq!(parse_chord("Ctrl"), None);
        assert_eq!(parse_chord("Ctrl+Foo"), None);
        assert_eq!(parse_chord("Ctrl+Shift"), None);
    }

    #[test]
    fn parse_chord_rejects_meta() {
        assert_eq!(parse_chord("Meta+C"), None);
        assert_eq!(parse_chord("Cmd+C"), None);
        assert_eq!(parse_chord("Command+C"), None);
    }

    #[test]
    fn parse_chord_named_and_symbol_keys() {
        assert_eq!(parse_chord("Ctrl+PageDown").unwrap().key, Key::PageDown);
        assert_eq!(parse_chord("Ctrl+Plus").unwrap().key, Key::Plus);
        assert_eq!(parse_chord("F11").unwrap().key, Key::F11);
        assert_eq!(parse_chord("Ctrl+,").unwrap().key, Key::Comma);
        assert_eq!(parse_chord("Ctrl+Shift+ArrowUp").unwrap().key, Key::ArrowUp);
        assert_eq!(parse_chord("Ctrl+0").unwrap().key, Key::Num0);
    }

    // ── from_settings: custom table + fallback ─────────────────────────

    #[test]
    fn from_settings_custom_new_tab_chord() {
        let mut kb = KeybindSettings::default();
        kb.new_tab = "Ctrl+Shift+N".to_string();
        let table = KeybindTable::from_settings(&kb);

        // The new spec dispatches NewTab.
        assert_eq!(
            dispatch(&table, mods(true, true, false), Key::N),
            Some(AppAction::NewTab)
        );
        // The old default no longer maps to NewTab (Ctrl+Shift+T).
        assert_eq!(dispatch(&table, mods(true, true, false), Key::T), None);
    }

    #[test]
    fn from_settings_alt_bearing_close_tab() {
        let mut kb = KeybindSettings::default();
        kb.close_tab = "Alt+W".to_string();
        let table = KeybindTable::from_settings(&kb);

        // Alt-only W now closes the tab — settings chords win over the
        // `!alt` built-in guard.
        assert_eq!(
            dispatch(&table, mods(false, false, true), Key::W),
            Some(AppAction::CloseTab)
        );
    }

    #[test]
    fn from_settings_unparseable_falls_back_to_default() {
        let mut kb = KeybindSettings::default();
        kb.copy = "garbage".to_string();
        let table = KeybindTable::from_settings(&kb);
        // Falls back to the built-in default chord (Ctrl+Shift+C).
        assert_eq!(
            table.copy,
            Chord {
                ctrl: true,
                shift: true,
                alt: false,
                key: Key::C,
            }
        );
    }

    #[test]
    fn default_table_has_no_collisions() {
        assert!(KeybindTable::default().collisions().is_empty());
    }

    #[test]
    fn colliding_chords_are_detected_in_priority_order() {
        let mut kb = KeybindSettings::default();
        // next_tab and prev_tab both bound to Ctrl+Tab: next_tab is
        // matched first by `dispatch`, so prev_tab is the dead binding.
        kb.next_tab = "Ctrl+Tab".to_string();
        kb.prev_tab = "Ctrl+Tab".to_string();
        let table = KeybindTable::from_settings(&kb);
        assert_eq!(table.collisions(), vec![("next_tab", "prev_tab")]);
        // The colliding chord itself still fires the winner.
        assert_eq!(
            dispatch(&table, mods(true, false, false), Key::Tab),
            Some(AppAction::NextTab)
        );
    }

    #[test]
    fn clipboard_chord_colliding_with_tab_action_is_detected() {
        let mut kb = KeybindSettings::default();
        // copy is consumed by handle_special_chord before dispatch ever
        // runs, so copy wins over a tab action sharing the same chord.
        kb.new_tab = "Ctrl+Shift+C".to_string();
        let table = KeybindTable::from_settings(&kb);
        assert_eq!(table.collisions(), vec![("copy", "new_tab")]);
    }

    // ── view-level actions: dispatch on the default table ──────────────

    #[test]
    fn default_ctrl_shift_a_is_select_all() {
        assert_eq!(
            dispatch(&KeybindTable::default(), mods(true, true, false), Key::A),
            Some(AppAction::SelectAll)
        );
    }

    #[test]
    fn default_ctrl_shift_f_is_open_search() {
        assert_eq!(
            dispatch(&KeybindTable::default(), mods(true, true, false), Key::F),
            Some(AppAction::OpenSearch)
        );
    }

    #[test]
    fn from_settings_custom_search_chord() {
        let mut kb = KeybindSettings::default();
        kb.search = "Ctrl+Shift+K".to_string();
        let table = KeybindTable::from_settings(&kb);
        assert_eq!(
            dispatch(&table, mods(true, true, false), Key::K),
            Some(AppAction::OpenSearch)
        );
        // The old default no longer opens search.
        assert_eq!(dispatch(&table, mods(true, true, false), Key::F), None);
    }

    #[test]
    fn default_ctrl_shift_up_is_jump_to_prev_prompt() {
        assert_eq!(
            dispatch(
                &KeybindTable::default(),
                mods(true, true, false),
                Key::ArrowUp
            ),
            Some(AppAction::JumpToPrevPrompt)
        );
    }

    #[test]
    fn default_ctrl_shift_down_is_jump_to_next_prompt() {
        assert_eq!(
            dispatch(
                &KeybindTable::default(),
                mods(true, true, false),
                Key::ArrowDown
            ),
            Some(AppAction::JumpToNextPrompt)
        );
    }

    #[test]
    fn from_settings_custom_prompt_jump_chords() {
        let mut kb = KeybindSettings::default();
        kb.jump_to_prev_prompt = "Ctrl+Shift+J".to_string();
        kb.jump_to_next_prompt = "Ctrl+Shift+L".to_string();
        let table = KeybindTable::from_settings(&kb);
        assert_eq!(
            dispatch(&table, mods(true, true, false), Key::J),
            Some(AppAction::JumpToPrevPrompt)
        );
        assert_eq!(
            dispatch(&table, mods(true, true, false), Key::L),
            Some(AppAction::JumpToNextPrompt)
        );
        // The old defaults no longer fire.
        assert_eq!(
            dispatch(&table, mods(true, true, false), Key::ArrowUp),
            None
        );
        assert_eq!(
            dispatch(&table, mods(true, true, false), Key::ArrowDown),
            None
        );
    }

    #[test]
    fn default_ctrl_plus_is_zoom_in() {
        assert_eq!(
            dispatch(
                &KeybindTable::default(),
                mods(true, false, false),
                Key::Plus
            ),
            Some(AppAction::ZoomIn)
        );
    }

    #[test]
    fn default_ctrl_minus_is_zoom_out() {
        assert_eq!(
            dispatch(
                &KeybindTable::default(),
                mods(true, false, false),
                Key::Minus
            ),
            Some(AppAction::ZoomOut)
        );
    }

    #[test]
    fn default_ctrl_zero_is_zoom_reset() {
        // Ctrl+0 is now bound to ZoomReset (it was unbound in Phase 4-B,
        // see `ctrl_zero_is_not_a_jump`, which asserts it does NOT jump).
        assert_eq!(
            dispatch(
                &KeybindTable::default(),
                mods(true, false, false),
                Key::Num0
            ),
            Some(AppAction::ZoomReset)
        );
    }

    #[test]
    fn default_f11_is_toggle_fullscreen() {
        assert_eq!(
            dispatch(
                &KeybindTable::default(),
                mods(false, false, false),
                Key::F11
            ),
            Some(AppAction::ToggleFullscreen)
        );
    }

    #[test]
    fn default_ctrl_shift_b_is_toggle_tab_bar() {
        assert_eq!(
            dispatch(&KeybindTable::default(), mods(true, true, false), Key::B),
            Some(AppAction::ToggleTabBar)
        );
    }

    // ── from_settings: new fields resolve + fall back ──────────────────

    #[test]
    fn from_settings_resolves_view_action_fields() {
        let kb = KeybindSettings::default();
        let table = KeybindTable::from_settings(&kb);
        assert_eq!(
            table.select_all,
            Chord {
                ctrl: true,
                shift: true,
                alt: false,
                key: Key::A
            }
        );
        assert_eq!(
            table.zoom_in,
            Chord {
                ctrl: true,
                shift: false,
                alt: false,
                key: Key::Plus
            }
        );
        assert_eq!(
            table.zoom_out,
            Chord {
                ctrl: true,
                shift: false,
                alt: false,
                key: Key::Minus
            }
        );
        assert_eq!(
            table.zoom_reset,
            Chord {
                ctrl: true,
                shift: false,
                alt: false,
                key: Key::Num0
            }
        );
        assert_eq!(
            table.toggle_fullscreen,
            Chord {
                ctrl: false,
                shift: false,
                alt: false,
                key: Key::F11
            }
        );
        assert_eq!(
            table.toggle_tab_bar,
            Chord {
                ctrl: true,
                shift: true,
                alt: false,
                key: Key::B
            }
        );
    }

    #[test]
    fn from_settings_custom_zoom_in_chord() {
        let mut kb = KeybindSettings::default();
        kb.zoom_in = "Ctrl+Equals".to_string();
        let table = KeybindTable::from_settings(&kb);
        assert_eq!(
            dispatch(&table, mods(true, false, false), Key::Equals),
            Some(AppAction::ZoomIn)
        );
        // The old default no longer maps to ZoomIn.
        assert_eq!(dispatch(&table, mods(true, false, false), Key::Plus), None);
    }

    #[test]
    fn from_settings_unparseable_view_action_falls_back() {
        let mut kb = KeybindSettings::default();
        kb.toggle_fullscreen = "not a chord!!".to_string();
        kb.select_all = "Ctrl+Bogus".to_string();
        let table = KeybindTable::from_settings(&kb);
        // Each falls back to its built-in default spec.
        assert_eq!(
            table.toggle_fullscreen,
            Chord {
                ctrl: false,
                shift: false,
                alt: false,
                key: Key::F11
            }
        );
        assert_eq!(
            table.select_all,
            Chord {
                ctrl: true,
                shift: true,
                alt: false,
                key: Key::A
            }
        );
    }

    // ── parse_chord: the new default specs specifically ────────────────

    #[test]
    fn parse_chord_view_action_default_specs() {
        assert_eq!(parse_chord("Ctrl+Shift+A").unwrap().key, Key::A);
        assert_eq!(parse_chord("Ctrl+Plus").unwrap().key, Key::Plus);
        assert_eq!(parse_chord("Ctrl+Minus").unwrap().key, Key::Minus);
        assert_eq!(parse_chord("Ctrl+0").unwrap().key, Key::Num0);
        assert_eq!(parse_chord("F11").unwrap().key, Key::F11);
        assert_eq!(parse_chord("Ctrl+Shift+B").unwrap().key, Key::B);
    }
}
