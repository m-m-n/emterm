use super::*;

// ── Zoom: clamp + runtime font size ───────────────────────────────

#[test]
fn clamp_font_size_pt_bounds() {
    // Below the floor clamps up; above the ceiling clamps down; a
    // value inside the range is returned unchanged.
    assert_eq!(clamp_font_size_pt(1.0), FONT_SIZE_PT_MIN);
    assert_eq!(clamp_font_size_pt(1000.0), FONT_SIZE_PT_MAX);
    assert_eq!(clamp_font_size_pt(12.0), 12.0);
    assert_eq!(clamp_font_size_pt(FONT_SIZE_PT_MIN), FONT_SIZE_PT_MIN);
    assert_eq!(clamp_font_size_pt(FONT_SIZE_PT_MAX), FONT_SIZE_PT_MAX);
}

#[test]
fn zoom_seeds_runtime_size_from_settings() {
    let app = App::new();
    assert_eq!(app.runtime_font_size_pt, app.settings.font_size);
}

#[test]
fn zoom_in_increments_by_one_point() {
    let mut app = App::new();
    let before = app.runtime_font_size_pt;
    assert!(app.zoom_in());
    assert!((app.runtime_font_size_pt - (before + FONT_SIZE_PT_STEP)).abs() < f32::EPSILON);
}

#[test]
fn zoom_out_decrements_by_one_point() {
    let mut app = App::new();
    let before = app.runtime_font_size_pt;
    assert!(app.zoom_out());
    assert!((app.runtime_font_size_pt - (before - FONT_SIZE_PT_STEP)).abs() < f32::EPSILON);
}

#[test]
fn zoom_reset_restores_settings_size() {
    let mut app = App::new();
    let baseline = app.settings.font_size;
    let _ = app.zoom_in();
    let _ = app.zoom_in();
    assert!(app.zoom_reset());
    assert!((app.runtime_font_size_pt - baseline).abs() < f32::EPSILON);
    // Resetting again is a no-op (already at the baseline).
    assert!(!app.zoom_reset());
}

#[test]
fn zoom_clamps_at_ceiling_and_floor() {
    let mut app = App::new();
    // Drive up to the ceiling; the step that would exceed it returns
    // false (no change) once clamped.
    app.runtime_font_size_pt = FONT_SIZE_PT_MAX;
    assert!(!app.zoom_in(), "already at ceiling: no change expected");
    assert_eq!(app.runtime_font_size_pt, FONT_SIZE_PT_MAX);
    // Same at the floor.
    app.runtime_font_size_pt = FONT_SIZE_PT_MIN;
    assert!(!app.zoom_out(), "already at floor: no change expected");
    assert_eq!(app.runtime_font_size_pt, FONT_SIZE_PT_MIN);
}

#[test]
fn set_font_size_pt_no_change_returns_false() {
    let mut app = App::new();
    let cur = app.runtime_font_size_pt;
    assert!(!app.set_font_size_pt(cur));
}

// ── apply_settings ─────────────────────────────────────────

#[test]
fn apply_settings_rebuilds_keybinds_and_locale() {
    let mut app = App::new();
    let mut new = Settings::default();
    new.keybinds.new_tab = "Ctrl+Shift+N".to_string();
    new.language = crate::settings::Language::Ja;

    app.apply_settings(new);
    let expected = crate::ui::keybinds::parse_chord("Ctrl+Shift+N").unwrap();
    assert_eq!(app.keybinds.new_tab, expected);
    assert_eq!(app.locale, crate::i18n::Locale::Ja);
}

#[test]
fn apply_settings_font_size_change_requests_resize_and_tracks_runtime() {
    let mut app = App::new();
    let mut new = Settings::default();
    new.font_size = 18.0;

    assert!(
        app.apply_settings(new),
        "font size change needs a grid reshape"
    );
    assert_eq!(app.runtime_font_size_pt, 18.0);
    assert_eq!(app.settings.font_size, 18.0);
}

#[test]
fn apply_settings_behavior_only_change_needs_no_resize() {
    let mut app = App::new();
    let mut new = Settings::default();
    new.scroll_speed = 9;
    new.copy_on_select = true;

    assert!(!app.apply_settings(new));
    assert_eq!(app.settings.scroll_speed, 9);
    assert!(app.settings.copy_on_select);
}

#[test]
fn apply_settings_clamps_to_save_ranges() {
    let mut app = App::new();
    let mut new = Settings::default();
    new.font_size = 500.0;
    new.scroll_speed = 99;

    app.apply_settings(new);
    assert_eq!(app.settings.font_size, 32.0);
    assert_eq!(app.settings.scroll_speed, 10);
}

#[test]
fn apply_settings_updates_tab_theme_and_fold_gate() {
    let mut app = App::new();
    app.spawn_initial_tab();
    let mut new = Settings::default();
    new.terminal_color_scheme = "dracula".to_string();
    new.fold_enabled = false;
    new.cursor_blink = false;

    app.apply_settings(new);
    let theme = app.tabs[0].theme.lock().clone();
    let expected = crate::render::theme::Theme::from_settings(app.settings.as_ref());
    assert_eq!(
        theme.bg, expected.bg,
        "dracula background applied to live tab"
    );
    assert!(
        !app.tabs[0].folds.is_enabled(),
        "fold gate pushed into live manager"
    );
}

#[test]
fn apply_settings_preserves_active_cursor_override_when_scheme_unchanged() {
    // AC-2: with an active OSC 12 override, a settings apply that does
    // NOT change the color scheme leaves the resolved cursor color at
    // the OSC value.
    let mut app = App::new();
    app.spawn_initial_tab();
    {
        let mut theme = app.tabs[0].theme.lock();
        assert!(theme.apply_osc(12, "rgb:aa/bb/cc"));
    }
    let mut new = Settings::default();
    new.cursor_blink = false; // unrelated change; scheme stays default

    app.apply_settings(new);

    let theme = app.tabs[0].theme.lock();
    assert_eq!(
        theme.cursor_fg,
        crate::render::theme::Rgb(0xaa, 0xbb, 0xcc),
        "override survives a settings apply that keeps the scheme"
    );
    assert!(theme.cursor_fg_override_active);
}

#[test]
fn apply_settings_scheme_change_with_active_override_updates_scheme_baseline_only() {
    // AC-3: with an active OSC 12 override, a settings apply that
    // CHANGES the color scheme updates `scheme_cursor_fg` to the new
    // scheme's cursor color while the resolved cursor color stays at
    // the OSC value; a subsequent OSC 112 restores the NEW scheme's
    // cursor color.
    let mut app = App::new();
    app.spawn_initial_tab();
    {
        let mut theme = app.tabs[0].theme.lock();
        assert!(theme.apply_osc(12, "rgb:aa/bb/cc"));
    }
    let mut new = Settings::default();
    new.terminal_color_scheme = "dracula".to_string();

    app.apply_settings(new);

    let expected_scheme_cursor =
        crate::render::theme::Theme::from_settings(app.settings.as_ref()).scheme_cursor_fg;
    {
        let theme = app.tabs[0].theme.lock();
        assert_eq!(
            theme.cursor_fg,
            crate::render::theme::Rgb(0xaa, 0xbb, 0xcc),
            "override still wins over the new scheme"
        );
        assert_eq!(
            theme.scheme_cursor_fg, expected_scheme_cursor,
            "scheme baseline updated to dracula's cursor color"
        );
        assert!(theme.cursor_fg_override_active);
    }

    // A subsequent OSC 112 restores the NEW scheme's cursor color, not
    // the old (default) scheme's.
    let mut theme = app.tabs[0].theme.lock();
    assert!(theme.apply_osc(112, ""));
    assert_eq!(theme.cursor_fg, expected_scheme_cursor);
}

#[test]
fn apply_settings_with_no_active_override_resolves_scheme_cursor_color() {
    // AC-4: with no active override, a settings apply resolves the
    // cursor color to the (possibly new) scheme cursor color — same as
    // today.
    let mut app = App::new();
    app.spawn_initial_tab();
    let mut new = Settings::default();
    new.terminal_color_scheme = "monokai".to_string();

    app.apply_settings(new);

    let expected = crate::render::theme::Theme::from_settings(app.settings.as_ref());
    let theme = app.tabs[0].theme.lock();
    assert_eq!(theme.cursor_fg, expected.cursor_fg);
    assert!(!theme.cursor_fg_override_active);
}

#[test]
fn apply_settings_updates_cursor_style_and_blink_on_every_tab() {
    // AC-3: applying new settings with `cursor_style: underline` /
    // `cursor_blink: false` updates EVERY existing tab's core so
    // `get_cursor_style()` = 1 and `get_cursor_blink()` = false.
    let mut app = App::new();
    app.spawn_initial_tab();
    app.spawn_initial_tab();
    let mut new = Settings::default();
    new.cursor_style = crate::settings::CursorStyle::Underline;
    new.cursor_blink = false;

    app.apply_settings(new);

    for tab in &app.tabs {
        let core = tab.core.lock();
        assert_eq!(core.get_cursor_style(), 1);
        assert!(!core.get_cursor_blink());
    }
}
