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
