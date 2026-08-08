use super::*;

/// Bare single-letter [`PrefixChord`] (no modifiers). Test-only: the defaults
/// moved to `Ctrl`-modified chords, but custom-binding tests still exercise
/// the bare follow-up path.
const fn bare_letter(c: char) -> PrefixChord {
    PrefixChord {
        ctrl: false,
        shift: false,
        alt: false,
        key: KeySym::Letter(c),
    }
}

/// Helper: the default Ctrl+Z prefix as a `KeyInput`.
fn prefix() -> KeyInput {
    KeyInput::ctrl_letter('z')
}

#[test]
fn format_chord_matches_settings_string_form() {
    assert_eq!(format_chord(&ctrl_letter('t')), "Ctrl+T");
    assert_eq!(format_chord(&PrefixChord::default()), "Ctrl+Z");
}

#[test]
fn format_chord_round_trips_through_parse() {
    for (_, chord) in DEFAULT_ACTION_BINDINGS {
        let s = format_chord(chord);
        assert_eq!(parse_prefix_key(&s).as_ref(), Some(chord), "round-trip {s}");
    }
}

#[test]
fn default_action_bindings_as_strings_exposes_ssot_in_order() {
    let exposed = default_action_bindings_as_strings();
    assert_eq!(
        exposed,
        vec![
            ("detach", "Ctrl+D".to_string()),
            ("new-window", "Ctrl+C".to_string()),
            ("next-window", "Ctrl+N".to_string()),
            ("prev-window", "Ctrl+P".to_string()),
            ("rename-window", "Ctrl+R".to_string()),
            ("move-window", "Ctrl+T".to_string()),
            ("toggle-window-sidebar", "Ctrl+W".to_string()),
            ("next-agent-window", "Ctrl+A".to_string()),
        ]
    );
}

/// AC-1: `toggle-window-sidebar` appears in the default action bindings
/// with chord Ctrl+W.
#[test]
fn toggle_window_sidebar_default_chord_is_ctrl_w() {
    assert_eq!(
        default_action_chord("toggle-window-sidebar"),
        Some(ctrl_letter('w'))
    );
    assert_eq!(
        ActionBindings::default().toggle_window_sidebar,
        ctrl_letter('w')
    );
}

/// mux-agent-tab-cycle task0001 AC-1: `next-agent-window` appears in
/// the default action bindings with chord Ctrl+A.
#[test]
fn next_agent_window_default_chord_is_ctrl_a() {
    assert_eq!(
        default_action_chord("next-agent-window"),
        Some(ctrl_letter('a'))
    );
    assert_eq!(
        ActionBindings::default().next_agent_window,
        ctrl_letter('a')
    );
}

// ── TS-prefix-1: arming + follow-up actions ─────────────────────────

#[test]
fn ctrl_z_arms_the_latch_without_emitting_action() {
    let mut l = Latch::default();
    let now = Instant::now();
    assert_eq!(l.observe(&prefix(), now), PrefixAction::None);
    assert!(l.is_armed());
}

#[test]
fn ctrl_z_then_ctrl_n_emits_next_window() {
    let mut l = Latch::default();
    let t0 = Instant::now();
    let _ = l.observe(&prefix(), t0);
    let t1 = t0 + Duration::from_millis(50);
    assert_eq!(
        l.observe(&KeyInput::ctrl_letter('n'), t1),
        PrefixAction::NextWindow
    );
    assert!(!l.is_armed());
}

#[test]
fn ctrl_z_then_ctrl_p_emits_prev_window() {
    let mut l = Latch::default();
    let t0 = Instant::now();
    let _ = l.observe(&prefix(), t0);
    assert_eq!(
        l.observe(&KeyInput::ctrl_letter('p'), t0 + Duration::from_millis(50)),
        PrefixAction::PrevWindow
    );
}

#[test]
fn ctrl_z_then_ctrl_d_emits_detach() {
    let mut l = Latch::default();
    let t0 = Instant::now();
    let _ = l.observe(&prefix(), t0);
    assert_eq!(
        l.observe(&KeyInput::ctrl_letter('d'), t0 + Duration::from_millis(50)),
        PrefixAction::Detach
    );
}

#[test]
fn ctrl_z_then_digit_emits_select_window() {
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
    assert_eq!(
        l.observe(&KeyInput::ctrl_letter('n'), later),
        PrefixAction::None
    );
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
fn parses_default_ctrl_z() {
    assert_eq!(parse_prefix_key("Ctrl+Z"), Some(PrefixChord::default()));
    assert_eq!(parse_prefix_key("ctrl+z"), Some(PrefixChord::default()));
    assert_eq!(
        parse_prefix_key("CONTROL + Z"),
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
fn default_bindings_map_ctrl_c_r_t() {
    let mut l = Latch::default();
    assert_eq!(
        arm_then(&mut l, KeyInput::ctrl_letter('c')),
        PrefixAction::NewWindow
    );
    let mut l = Latch::default();
    assert_eq!(
        arm_then(&mut l, KeyInput::ctrl_letter('r')),
        PrefixAction::RenameWindow
    );
    let mut l = Latch::default();
    assert_eq!(
        arm_then(&mut l, KeyInput::ctrl_letter('t')),
        PrefixAction::MoveWindow
    );
}

#[test]
fn default_bindings_map_ctrl_d_n_p_and_digits() {
    let mut l = Latch::default();
    assert_eq!(
        arm_then(&mut l, KeyInput::ctrl_letter('d')),
        PrefixAction::Detach
    );
    let mut l = Latch::default();
    assert_eq!(
        arm_then(&mut l, KeyInput::ctrl_letter('n')),
        PrefixAction::NextWindow
    );
    let mut l = Latch::default();
    assert_eq!(
        arm_then(&mut l, KeyInput::ctrl_letter('p')),
        PrefixAction::PrevWindow
    );
    let mut l = Latch::default();
    assert_eq!(
        arm_then(&mut l, KeyInput::digit(3)),
        PrefixAction::SelectWindow(3)
    );
}

/// AC-1: the Ctrl+Z Ctrl+W chord dispatches `ToggleWindowSidebar`
/// through the same latch path as the other default follow-ups.
#[test]
fn default_binding_maps_ctrl_w_to_toggle_window_sidebar() {
    let mut l = Latch::default();
    assert_eq!(
        arm_then(&mut l, KeyInput::ctrl_letter('w')),
        PrefixAction::ToggleWindowSidebar
    );
}

/// mux-agent-tab-cycle task0001 AC-1: the Ctrl+Z Ctrl+A chord
/// dispatches `NextAgentWindow` through the same latch path as the
/// other default follow-ups.
#[test]
fn default_binding_maps_ctrl_a_to_next_agent_window() {
    let mut l = Latch::default();
    assert_eq!(
        arm_then(&mut l, KeyInput::ctrl_letter('a')),
        PrefixAction::NextAgentWindow
    );
}

/// mux-agent-tab-cycle task0001 AC-1: a user override of
/// `settings.mux.keybinds["next-agent-window"]` wins over the Ctrl+A
/// default, and the default no longer fires once overridden.
#[test]
fn custom_next_agent_window_binding_overrides_default() {
    let mut map = std::collections::HashMap::new();
    map.insert("next-agent-window".to_string(), bare_letter('g'));
    let bindings = ActionBindings::from_settings_map(&map);
    let mut l = Latch::with_bindings(PrefixChord::default(), DEFAULT_ARMED_TIMEOUT, bindings);
    assert_eq!(
        arm_then(&mut l, KeyInput::letter('g')),
        PrefixAction::NextAgentWindow
    );

    let mut l_stale = Latch::with_bindings(
        PrefixChord::default(),
        DEFAULT_ARMED_TIMEOUT,
        ActionBindings::from_settings_map(&map),
    );
    assert_eq!(
        arm_then(&mut l_stale, KeyInput::ctrl_letter('a')),
        PrefixAction::None,
        "the overridden default (Ctrl+A) no longer fires"
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

/// AC-4: a user override of `toggle-window-sidebar` rebinds only that
/// action — the other six keep firing their own (unrelated) defaults.
#[test]
fn custom_toggle_window_sidebar_binding_does_not_affect_other_actions() {
    let mut map = std::collections::HashMap::new();
    map.insert("toggle-window-sidebar".to_string(), bare_letter('s'));
    let bindings = ActionBindings::from_settings_map(&map);
    let mut l = Latch::with_bindings(PrefixChord::default(), DEFAULT_ARMED_TIMEOUT, bindings);
    // New chord fires the action.
    assert_eq!(
        arm_then(&mut l, KeyInput::letter('s')),
        PrefixAction::ToggleWindowSidebar
    );
    // Old default (Ctrl+W) no longer fires it (the binding was rewritten).
    let mut l_stale = Latch::with_bindings(
        PrefixChord::default(),
        DEFAULT_ARMED_TIMEOUT,
        ActionBindings::from_settings_map(&map),
    );
    assert_eq!(
        arm_then(&mut l_stale, KeyInput::ctrl_letter('w')),
        PrefixAction::None
    );
    // The other six actions are untouched by the override.
    let checks: &[(char, PrefixAction)] = &[
        ('d', PrefixAction::Detach),
        ('c', PrefixAction::NewWindow),
        ('n', PrefixAction::NextWindow),
        ('p', PrefixAction::PrevWindow),
        ('r', PrefixAction::RenameWindow),
        ('t', PrefixAction::MoveWindow),
    ];
    for (key, want) in checks {
        let mut l_other = Latch::with_bindings(
            PrefixChord::default(),
            DEFAULT_ARMED_TIMEOUT,
            ActionBindings::from_settings_map(&map),
        );
        assert_eq!(
            arm_then(&mut l_other, KeyInput::ctrl_letter(*key)),
            *want,
            "action for ctrl+{key} must stay at its default"
        );
    }
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
    assert_eq!(b.detach, ctrl_letter('d'));
    assert_eq!(b.new_window, ctrl_letter('c'));
    assert_eq!(b.move_window, ctrl_letter('t'));
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
        crate::mux::prefix::parse_prefix_key("Ctrl+D").unwrap(),
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
                crate::mux::prefix::parse_prefix_key("Ctrl+D").unwrap(),
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
/// detach under a bare binding, even though the WebView fires it. The
/// defaults moved to `Ctrl`-modified chords, so the bare binding is set
/// explicitly here.
#[test]
fn bare_binding_ignores_modifiers_like_webview() {
    let bare_detach = || {
        let mut m = std::collections::HashMap::new();
        m.insert(
            "detach".to_string(),
            crate::mux::prefix::parse_prefix_key("d").unwrap(),
        );
        ActionBindings::from_settings_map(&m)
    };
    // `Ctrl+d` after the prefix fires `Detach` because `detach = "d"`
    // is a bare binding (no modifiers in the binding spec).
    let mut l = Latch::with_bindings(PrefixChord::default(), DEFAULT_ARMED_TIMEOUT, bare_detach());
    assert_eq!(
        arm_then(&mut l, KeyInput::ctrl_letter('d')),
        PrefixAction::Detach
    );
    // Same for `Shift+d`.
    let mut l2 = Latch::with_bindings(PrefixChord::default(), DEFAULT_ARMED_TIMEOUT, bare_detach());
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
    let mut l3 = Latch::with_bindings(PrefixChord::default(), DEFAULT_ARMED_TIMEOUT, bare_detach());
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
        crate::mux::prefix::parse_prefix_key("Ctrl+D").unwrap(),
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
    let comma = crate::mux::prefix::parse_prefix_key("Comma").unwrap();
    let period = crate::mux::prefix::parse_prefix_key("Period").unwrap();
    let slash = crate::mux::prefix::parse_prefix_key("Slash").unwrap();
    let backslash = crate::mux::prefix::parse_prefix_key("Backslash").unwrap();
    let minus = crate::mux::prefix::parse_prefix_key("Minus").unwrap();
    let semicolon = crate::mux::prefix::parse_prefix_key("Semicolon").unwrap();
    assert_eq!(comma.key, KeySym::Comma);
    assert_eq!(period.key, KeySym::Period);
    assert_eq!(slash.key, KeySym::Slash);
    assert_eq!(backslash.key, KeySym::Backslash);
    assert_eq!(minus.key, KeySym::Minus);
    assert_eq!(semicolon.key, KeySym::Semicolon);
    // Case-insensitive.
    let lc = crate::mux::prefix::parse_prefix_key("comma").unwrap();
    assert_eq!(lc.key, KeySym::Comma);
    // Modifier + name form (matches the WebView UI's `Ctrl+Comma` save shape).
    let ctrl_comma = crate::mux::prefix::parse_prefix_key("Ctrl+Comma").unwrap();
    assert!(ctrl_comma.ctrl);
    assert_eq!(ctrl_comma.key, KeySym::Comma);
}
