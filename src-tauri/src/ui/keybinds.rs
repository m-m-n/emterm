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
    pub profile_selector: Chord,
    pub new_tab_global: Chord,
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
    pub open_settings: Chord,
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
            profile_selector: resolve("profile_selector", &kb.profile_selector),
            new_tab_global: resolve("new_tab_global", &kb.new_tab_global),
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
            open_settings: resolve("open_settings", &kb.open_settings),
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
            ("profile_selector", self.profile_selector),
            ("new_tab_global", self.new_tab_global),
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
            ("open_settings", self.open_settings),
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
        "open_settings" => "Ctrl+,",
        "profile_selector" => "Ctrl+Shift+P",
        "new_tab_global" => "Ctrl+Shift+G",
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
    //    `profile_selector` and `new_tab_global` are checked before
    //    `new_tab` so they win when the user maps several actions to the
    //    same combination (WebView parity: `keyboard-handler.ts` matches
    //    in this order).
    if chord == table.profile_selector {
        return Some(AppAction::OpenProfileSelector);
    }
    if chord == table.new_tab_global {
        return Some(AppAction::NewTabGlobal);
    }
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
    if chord == table.open_settings {
        return Some(AppAction::OpenSettings);
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
mod tests;
