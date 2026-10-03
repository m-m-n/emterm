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

// ── format_chord: canonical shortcut label text ────────────────────
//
// new-tab-menu-shortcut-hints task0001 AC-1 / AC-2 (FR5, NFR4).

/// Build a table from the defaults with `edit` applied to the keybind
/// settings (the pattern the `from_settings_*` tests above use).
fn table_with(edit: impl FnOnce(&mut KeybindSettings)) -> KeybindTable {
    let mut kb = KeybindSettings::default();
    edit(&mut kb);
    KeybindTable::from_settings(&kb)
}

fn chord(ctrl: bool, shift: bool, alt: bool, key: Key) -> Chord {
    Chord {
        ctrl,
        shift,
        alt,
        key,
    }
}

// AC-1: default table labels for the two chooser actions.
#[test]
fn format_chord_default_table_new_tab_global_and_new_tab() {
    let table = KeybindTable::default();
    assert_eq!(format_chord(&table.new_tab_global), "Ctrl+Shift+G");
    assert_eq!(format_chord(&table.new_tab), "Ctrl+Shift+T");
}

// AC-1: the label follows the resolved chord, not the spec's spelling.
#[test]
fn format_chord_lowercase_spec_is_canonicalized() {
    let table = table_with(|kb| kb.new_tab_global = "ctrl+shift+g".to_string());
    assert_eq!(format_chord(&table.new_tab_global), "Ctrl+Shift+G");
}

// AC-1: modifiers in the fixed order Ctrl, Shift, Alt.
#[test]
fn format_chord_orders_modifiers_ctrl_shift_alt_then_key() {
    assert_eq!(
        format_chord(&chord(true, true, true, Key::T)),
        "Ctrl+Shift+Alt+T"
    );
}

// AC-1: only the enabled modifiers appear; a bare key has no prefix.
#[test]
fn format_chord_omits_disabled_modifiers() {
    assert_eq!(format_chord(&chord(true, false, false, Key::T)), "Ctrl+T");
    assert_eq!(format_chord(&chord(false, true, false, Key::T)), "Shift+T");
    assert_eq!(format_chord(&chord(false, false, true, Key::T)), "Alt+T");
    assert_eq!(
        format_chord(&chord(true, false, true, Key::T)),
        "Ctrl+Alt+T"
    );
    assert_eq!(format_chord(&chord(false, false, false, Key::F11)), "F11");
}

// AC-1: an unparseable spec resolves to the built-in default, and the
// label shows that resolved chord.
#[test]
fn format_chord_unparseable_new_tab_spec_shows_fallback_default() {
    let table = table_with(|kb| kb.new_tab = "not a chord!!".to_string());
    assert_eq!(format_chord(&table.new_tab), "Ctrl+Shift+T");
}

// AC-1: no symbol alias is emitted; word tokens are CamelCase.
#[test]
fn format_chord_uses_word_tokens_not_symbols() {
    assert_eq!(
        format_chord(&chord(true, false, false, Key::Plus)),
        "Ctrl+Plus"
    );
    assert_eq!(
        format_chord(&chord(true, false, false, Key::Minus)),
        "Ctrl+Minus"
    );
    assert_eq!(
        format_chord(&chord(true, false, false, Key::Comma)),
        "Ctrl+Comma"
    );
    assert_eq!(
        format_chord(&chord(true, false, false, Key::Backslash)),
        "Ctrl+Backslash"
    );
    assert_eq!(
        format_chord(&chord(true, false, false, Key::PageDown)),
        "Ctrl+PageDown"
    );
    assert_eq!(
        format_chord(&chord(true, true, false, Key::ArrowUp)),
        "Ctrl+Shift+ArrowUp"
    );
    assert_eq!(
        format_chord(&chord(true, false, false, Key::Num0)),
        "Ctrl+0"
    );
}

/// Every chord of a table, for round-trip checks.
fn all_chords(t: &KeybindTable) -> Vec<Chord> {
    vec![
        t.copy,
        t.paste,
        t.profile_selector,
        t.new_tab_global,
        t.new_tab,
        t.close_tab,
        t.next_tab,
        t.prev_tab,
        t.select_all,
        t.search,
        t.jump_to_prev_prompt,
        t.jump_to_next_prompt,
        t.zoom_in,
        t.zoom_out,
        t.zoom_reset,
        t.toggle_fullscreen,
        t.toggle_tab_bar,
        t.open_settings,
    ]
}

// AC-2: parsing the label returns the original chord for every chord of
// the default table.
#[test]
fn format_chord_round_trips_every_default_table_chord() {
    for c in all_chords(&KeybindTable::default()) {
        let label = format_chord(&c);
        assert_eq!(parse_chord(&label), Some(c), "label {label:?}");
    }
}

// AC-2: the named samples the SPEC calls out.
#[test]
fn format_chord_round_trips_named_samples() {
    for c in [
        chord(true, false, false, Key::PageDown),
        chord(true, true, false, Key::ArrowUp),
        chord(true, false, false, Key::Plus),
        chord(false, false, false, Key::F11),
        chord(true, false, false, Key::Num7),
        chord(true, false, false, Key::Comma),
    ] {
        let label = format_chord(&c);
        assert_eq!(parse_chord(&label), Some(c), "label {label:?}");
    }
}

/// Every main-key token `parse_main_key` accepts, as the word / letter /
/// digit spellings (symbol aliases are listed separately because they
/// parse to the same keys).
fn parseable_main_key_tokens() -> Vec<String> {
    let mut tokens: Vec<String> = Vec::new();
    tokens.extend(('a'..='z').map(|c| c.to_string()));
    tokens.extend(('0'..='9').map(|c| c.to_string()));
    tokens.extend((1..=20).map(|n| format!("f{n}")));
    tokens.extend(
        [
            "plus",
            "minus",
            "comma",
            "period",
            "slash",
            "backslash",
            "space",
            "enter",
            "escape",
            "tab",
            "backspace",
            "delete",
            "insert",
            "arrowup",
            "arrowdown",
            "arrowleft",
            "arrowright",
            "home",
            "end",
            "pageup",
            "pagedown",
            "equals",
            "semicolon",
            "colon",
            // Symbol aliases that `parse_chord` can read inside a spec
            // (`+` cannot: specs are split on it).
            "-",
            ",",
            ".",
            "/",
            "\\",
            "=",
            ";",
            ":",
        ]
        .map(String::from),
    );
    tokens
}

// AC-2: every main key `parse_chord` can produce survives
// parse -> format -> parse under every modifier combination.
#[test]
fn format_chord_round_trips_every_parseable_main_key() {
    for token in parseable_main_key_tokens() {
        let key = parse_chord(&token)
            .unwrap_or_else(|| panic!("token {token:?} must parse"))
            .key;
        for bits in 0..8u8 {
            let c = chord(bits & 1 != 0, bits & 2 != 0, bits & 4 != 0, key);
            let label = format_chord(&c);
            assert_eq!(
                parse_chord(&label),
                Some(c),
                "token {token:?} chord {c:?} label {label:?}"
            );
        }
    }
}

// The formatter never panics and never returns an empty label, even for
// a key `parse_chord` cannot produce (no round-trip guarantee there).
#[test]
fn format_chord_non_empty_for_every_egui_key() {
    for &key in Key::ALL {
        let label = format_chord(&chord(false, false, false, key));
        assert!(!label.is_empty(), "key {key:?}");
        // A label that does parse must never come back as another chord.
        if let Some(parsed) = parse_chord(&label) {
            assert_eq!(parsed, chord(false, false, false, key), "label {label:?}");
        }
    }
}

// ── reachable shortcut labels for the new-tab chooser ──────────────
//
// new-tab-menu-shortcut-hints task0001 AC-4 (FR6): a label is withheld
// exactly when an action earlier in the runtime match priority owns the
// same resolved chord.

#[test]
fn reachable_labels_default_table_shows_both() {
    let table = KeybindTable::default();
    assert_eq!(
        table.new_tab_global_label().as_deref(),
        Some("Ctrl+Shift+G")
    );
    assert_eq!(table.new_tab_label().as_deref(), Some("Ctrl+Shift+T"));
}

#[test]
fn reachable_labels_follow_the_resolved_table() {
    let table = table_with(|kb| {
        kb.new_tab_global = "Ctrl+Alt+G".to_string();
        kb.new_tab = "Ctrl+Alt+N".to_string();
    });
    assert_eq!(table.new_tab_global_label().as_deref(), Some("Ctrl+Alt+G"));
    assert_eq!(table.new_tab_label().as_deref(), Some("Ctrl+Alt+N"));
}

#[test]
fn new_tab_global_label_hidden_by_copy() {
    let table = table_with(|kb| kb.new_tab_global = kb.copy.clone());
    assert_eq!(table.new_tab_global_label(), None);
}

#[test]
fn new_tab_global_label_hidden_by_paste() {
    let table = table_with(|kb| kb.new_tab_global = kb.paste.clone());
    assert_eq!(table.new_tab_global_label(), None);
}

#[test]
fn new_tab_global_label_hidden_by_profile_selector() {
    let table = table_with(|kb| kb.new_tab_global = kb.profile_selector.clone());
    assert_eq!(table.new_tab_global_label(), None);
}

#[test]
fn new_tab_label_hidden_by_copy() {
    let table = table_with(|kb| kb.new_tab = kb.copy.clone());
    assert_eq!(table.new_tab_label(), None);
}

#[test]
fn new_tab_label_hidden_by_paste() {
    let table = table_with(|kb| kb.new_tab = kb.paste.clone());
    assert_eq!(table.new_tab_label(), None);
}

#[test]
fn new_tab_label_hidden_by_profile_selector() {
    let table = table_with(|kb| kb.new_tab = kb.profile_selector.clone());
    assert_eq!(table.new_tab_label(), None);
}

#[test]
fn new_tab_label_hidden_by_new_tab_global() {
    let table = table_with(|kb| kb.new_tab = kb.new_tab_global.clone());
    assert_eq!(table.new_tab_label(), None);
    // The winner keeps its label.
    assert_eq!(
        table.new_tab_global_label().as_deref(),
        Some("Ctrl+Shift+G")
    );
}

// A collision with a LOWER-priority action does not hide the label.
#[test]
fn new_tab_label_kept_when_only_a_lower_priority_action_shares_the_chord() {
    let table = table_with(|kb| kb.new_tab = kb.close_tab.clone());
    assert_eq!(table.collisions(), vec![("new_tab", "close_tab")]);
    assert_eq!(table.new_tab_label().as_deref(), Some("Ctrl+Shift+W"));
}

#[test]
fn new_tab_global_label_kept_when_only_a_lower_priority_action_shares_the_chord() {
    let table = table_with(|kb| kb.new_tab_global = kb.new_tab.clone());
    // new_tab_global outranks new_tab, so new_tab is the dead binding.
    assert_eq!(
        table.new_tab_global_label().as_deref(),
        Some("Ctrl+Shift+T")
    );
    let table = table_with(|kb| kb.new_tab_global = kb.close_tab.clone());
    assert_eq!(
        table.new_tab_global_label().as_deref(),
        Some("Ctrl+Shift+W")
    );
}

// The query leaves the collision report (and so the warn logging built
// on it) exactly as it was.
#[test]
fn label_queries_do_not_change_the_collision_report() {
    let table = table_with(|kb| kb.new_tab = kb.new_tab_global.clone());
    let before = table.collisions();
    let _ = table.new_tab_global_label();
    let _ = table.new_tab_label();
    assert_eq!(table.collisions(), before);
    assert_eq!(before, vec![("new_tab_global", "new_tab")]);
}
