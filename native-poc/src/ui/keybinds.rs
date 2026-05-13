//! Central keybind dispatch.
//!
//! Phase 4-B implements a pure mapping from `(egui::Modifiers,
//! egui::Key)` to [`AppAction`]. The dispatcher is intentionally
//! state-free so it can be exercised directly from unit tests by
//! constructing synthetic `(mods, key)` pairs — no egui context, no
//! tabs vector, no PTY required.
//!
//! Bindings:
//!
//! | Chord                  | Action                |
//! |------------------------|-----------------------|
//! | Ctrl+Shift+T           | `NewTab`              |
//! | Ctrl+Shift+W           | `CloseTab`            |
//! | Ctrl+Tab               | `NextTab`             |
//! | Ctrl+Shift+Tab         | `PrevTab`             |
//! | Ctrl+1 .. Ctrl+9       | `JumpTab(n)` (1-based)|
//!
//! Every other chord returns `None` and the caller should fall through
//! to the active PTY writer (or the existing Phase 4-A clipboard /
//! scrollback chord layer in `window_host.rs`).

use egui::{Key, Modifiers};

use super::AppAction;

/// Map an `(egui::Modifiers, egui::Key)` pair to an [`AppAction`].
///
/// Returns `None` for any chord that is not in the global keybind
/// table; the caller is expected to forward such inputs to the active
/// PTY.
///
/// The table considers the **logical** modifier flags (`ctrl`,
/// `shift`, `alt`). The `command` / `mac_cmd` fields are ignored
/// because Phase 4 targets Linux + Windows only.
pub fn dispatch(mods: Modifiers, key: Key) -> Option<AppAction> {
    // Strip platform-specific aliases we don't honor (mac_cmd). The
    // egui `command` flag aliases to ctrl on non-mac so we treat the
    // canonical `ctrl` bit as authoritative.
    let ctrl = mods.ctrl || mods.command;
    let shift = mods.shift;
    let alt = mods.alt;

    // Alt is never part of a Phase 4-B binding; bail to keep PTY
    // forwarding (e.g. Alt-prefixed escape sequences) intact.
    if alt {
        return None;
    }

    match (ctrl, shift, key) {
        // Tab roster mutations
        (true, true, Key::T) => Some(AppAction::NewTab),
        (true, true, Key::W) => Some(AppAction::CloseTab),

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
    // pairs through `dispatch` and assert AppAction equality.

    #[test]
    fn ctrl_shift_t_is_new_tab() {
        assert_eq!(
            dispatch(mods(true, true, false), Key::T),
            Some(AppAction::NewTab)
        );
    }

    #[test]
    fn ctrl_shift_w_is_close_tab() {
        assert_eq!(
            dispatch(mods(true, true, false), Key::W),
            Some(AppAction::CloseTab)
        );
    }

    #[test]
    fn ctrl_tab_is_next_tab() {
        assert_eq!(
            dispatch(mods(true, false, false), Key::Tab),
            Some(AppAction::NextTab)
        );
    }

    #[test]
    fn ctrl_shift_tab_is_prev_tab() {
        assert_eq!(
            dispatch(mods(true, true, false), Key::Tab),
            Some(AppAction::PrevTab)
        );
    }

    #[test]
    fn ctrl_digit_jumps_to_tab() {
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
                dispatch(mods(true, false, false), key),
                Some(AppAction::JumpTab(want)),
                "Ctrl+{want} should jump to tab {want}"
            );
        }
    }

    #[test]
    fn ctrl_zero_is_not_a_jump() {
        // Phase 4-B intentionally binds 1..=9; Ctrl+0 stays free for
        // future use (reset zoom / etc.) and falls through to the PTY.
        assert_eq!(dispatch(mods(true, false, false), Key::Num0), None);
    }

    #[test]
    fn ctrl_shift_digit_does_not_jump() {
        // Ctrl+Shift+1 must NOT trigger JumpTab(1); apps that bind
        // Ctrl+Shift+digit (e.g. tmux profiles) need passthrough.
        assert_eq!(dispatch(mods(true, true, false), Key::Num1), None);
    }

    #[test]
    fn alt_prefixed_chord_falls_through() {
        // Alt+Tab is window-manager territory; Alt+Shift+T must not
        // hijack the global keybind path either.
        assert_eq!(dispatch(mods(false, false, true), Key::Tab), None);
        assert_eq!(dispatch(mods(true, true, true), Key::T), None);
    }

    #[test]
    fn unbound_chord_returns_none() {
        // Plain "T", Ctrl+T (no Shift), Shift+T — all PTY-bound.
        assert_eq!(dispatch(mods(false, false, false), Key::T), None);
        assert_eq!(dispatch(mods(true, false, false), Key::T), None);
        assert_eq!(dispatch(mods(false, true, false), Key::T), None);
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
        assert_eq!(dispatch(m, Key::T), Some(AppAction::NewTab));
    }
}
