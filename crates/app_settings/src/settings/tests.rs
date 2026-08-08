use super::*;
use serde_json::json;

/// FR4/FR8b (mux-status-bar-removal task0001): a `settings.json`
/// written by an older eMterm build may still contain the retired
/// `mux.statusbar` object (the removed mux status-bar settings
/// schema). Loading it must not fail -- the obsolete key is silently
/// ignored (no `MuxSettings` field consumes it anymore) rather than
/// rejected.
#[test]
fn test_mux_settings_tolerates_stale_statusbar_key() {
    let json = r#"{
        "prefix": "ctrl+a",
        "statusbar": {
            "enabled": true,
            "left": "test",
            "right": "right",
            "commands": {
                "git_branch": {"executable": "/usr/bin/git-branch-name"}
            }
        }
    }"#;
    let settings: MuxSettings = serde_json::from_str(json).unwrap();
    assert_eq!(settings.prefix, "ctrl+a");
}

// ── window_sidebar_overlay (task0001 AC-1/AC-3/AC-4/AC-5) ───────────

#[test]
fn test_mux_settings_window_sidebar_overlay_missing_defaults_overlay() {
    // AC-1: a settings JSON without the field resolves to the overlay
    // display mode (`true`).
    let json = r#"{"prefix": "ctrl+b"}"#;
    let settings: MuxSettings = serde_json::from_str(json).unwrap();
    assert!(settings.window_sidebar_overlay);
}

#[test]
fn test_mux_settings_window_sidebar_overlay_explicit_false_is_persistent() {
    // AC-3: an explicit `false` resolves to the persistent display
    // mode, overriding the overlay default.
    let json = r#"{"window_sidebar_overlay": false}"#;
    let settings: MuxSettings = serde_json::from_str(json).unwrap();
    assert!(!settings.window_sidebar_overlay);
}

#[test]
fn test_mux_settings_window_sidebar_overlay_explicit_true_is_overlay() {
    // AC-4: `true` resolves to the overlay display mode.
    let json = r#"{"window_sidebar_overlay": true}"#;
    let settings: MuxSettings = serde_json::from_str(json).unwrap();
    assert!(settings.window_sidebar_overlay);
}

#[test]
fn test_mux_settings_window_sidebar_overlay_round_trips() {
    // AC-5: serializing default settings then re-loading them
    // round-trips the overlay display mode, and a round trip of an
    // explicitly-persistent value preserves the persistent mode.
    let default_settings = MuxSettings::default();
    let json = serde_json::to_string(&default_settings).unwrap();
    let restored: MuxSettings = serde_json::from_str(&json).unwrap();
    assert!(restored.window_sidebar_overlay);

    let mut persistent_settings = MuxSettings::default();
    persistent_settings.window_sidebar_overlay = false;
    let json = serde_json::to_string(&persistent_settings).unwrap();
    let restored: MuxSettings = serde_json::from_str(&json).unwrap();
    assert!(!restored.window_sidebar_overlay);
}

/// Full-coverage round-trip: the exhaustive struct literal (no
/// `..Default::default()`) forces a compile error here whenever a new
/// field is added, so the round-trip assertions stay complete. Lives in
/// this crate (moved from src-tauri's config tests) because the literal
/// must name the crate-private legacy fields.
#[test]
fn test_round_trip_preserves_all_fields() {
    let settings = AppSettings {
        font_size: 16,
        font_family_primary: "Fira Code".to_string(),
        font_family_secondary: "Noto Sans JP".to_string(),
        font_family: String::new(),
        _line_height: None,
        ui_theme: UiTheme::Dark,
        ui_theme_preset: UiThemePreset::Blue,
        terminal_color_scheme: "monokai".to_string(),
        padding: 8,
        scrollback_lines: 5000,
        show_scrollbar: ScrollbarMode::Always,
        show_tab_bar: false,
        shell_path: "/bin/zsh".to_string(),
        shell_args: vec!["--login".to_string(), "-i".to_string()],
        cursor_style: CursorStyle::Bar,
        cursor_blink: false,
        scroll_speed: 5,
        alternate_scroll_enabled: false,
        bell_action: BellAction::None,
        url_detection: false,
        copy_on_select: true,
        keybinds: KeybindSettings {
            copy: "Ctrl+C".to_string(),
            paste: "Ctrl+V".to_string(),
            ..KeybindSettings::default()
        },
        language: "ja".to_string(),
        ui_font_family: "Noto Sans".to_string(),
        custom_color_schemes: Vec::new(),
        profiles: vec![Profile {
            name: "Dev".to_string(),
            shell_path: "/bin/zsh".to_string(),
            shell_args: vec!["--login".to_string()],
            env_vars: "FOO=bar\nBAZ=qux".to_string(),
            working_directory: "/home/user/projects".to_string(),
            is_default: true,
            ssh_connection_name: String::new(),
            wsl_distro_name: String::new(),
        }],
        ssh_command_path: String::new(),
        ssh_connections: Vec::new(),
        sftp_max_concurrent_uploads: 4,
        clipboard_read_osc52: true,
        clipboard_max_size_osc52: 10 * 1024 * 1024,
        log_recording_enabled: false,
        markdown_theme_follow_ui: false,
        markdown_theme: UiTheme::Light,
        markdown_theme_preset: UiThemePreset::Green,
        markdown_body_font_family: "Noto Sans".to_string(),
        markdown_code_font_family: "Fira Code".to_string(),
        markdown_font_size: 16,
        fold_enabled: false,
        file_path_detection: false,
        bold_brightens_ansi_colors: false,
        middle_click_paste: false,
        shift_enter_behavior: ShiftEnterBehavior::KittyCsiU,
        shift_enter_as_alt_enter: None,
        ambiguous_width: false,
        editor_command: "vim +{line} {file}".to_string(),
        skk_mode: false,
        notification_enabled: false,
        tab_activity_indicator: false,
        notify_on_process_exit: false,
        notify_on_output: true,
        notify_on_bell: false,
        agent_status_notifications: false,
        agent_notify_on_done: false,
        agent_notify_on_blocked: false,
        mux: MuxSettings::default(),
        statusbar_enabled: true,
        statusbar_app_line1_left: "{git_branch}".to_string(),
        statusbar_app_line1_right: "{time}".to_string(),
        statusbar_app_line2_left: "line2".to_string(),
        statusbar_app_line2_right: "right2".to_string(),
        statusbar_time_format: "HH:mm".to_string(),
        statusbar_font_size: Some(11.0),
        statusbar_custom_commands: std::collections::HashMap::new(),
        statusbar_refresh_rates: std::collections::HashMap::new(),
    };

    let json = serde_json::to_string(&settings).unwrap();
    let restored: AppSettings = serde_json::from_str(&json).unwrap();

    assert_eq!(restored.font_size, 16);
    assert_eq!(restored.font_family_primary, "Fira Code");
    assert_eq!(restored.font_family_secondary, "Noto Sans JP");
    assert_eq!(restored.ui_theme, UiTheme::Dark);
    assert_eq!(restored.ui_theme_preset, UiThemePreset::Blue);
    assert_eq!(restored.terminal_color_scheme, "monokai");
    assert_eq!(restored.padding, 8);
    assert_eq!(restored.scrollback_lines, 5000);
    assert_eq!(restored.show_scrollbar, ScrollbarMode::Always);
    assert_eq!(restored.shell_path, "/bin/zsh");
    assert_eq!(restored.shell_args, vec!["--login", "-i"]);
    assert_eq!(restored.cursor_style, CursorStyle::Bar);
    assert!(!restored.cursor_blink);
    assert_eq!(restored.scroll_speed, 5);
    assert!(!restored.alternate_scroll_enabled);
    assert_eq!(restored.bell_action, BellAction::None);
    assert!(!restored.url_detection);
    assert!(restored.copy_on_select);
    assert!(!restored.file_path_detection);
    assert!(!restored.bold_brightens_ansi_colors);
    assert!(!restored.middle_click_paste);
    assert_eq!(restored.shift_enter_behavior, ShiftEnterBehavior::KittyCsiU);
    assert_eq!(restored.editor_command, "vim +{line} {file}");
    assert!(!restored.skk_mode);
    assert_eq!(restored.keybinds.copy, "Ctrl+C");
    assert_eq!(restored.keybinds.paste, "Ctrl+V");
    assert_eq!(restored.keybinds.select_all, "Ctrl+Shift+A");
    assert_eq!(restored.ui_font_family, "Noto Sans");
    assert_eq!(restored.language, "ja");
    assert!(!restored.show_tab_bar);
    assert!(!restored.markdown_theme_follow_ui);
    assert_eq!(restored.markdown_theme, UiTheme::Light);
    assert_eq!(restored.markdown_theme_preset, UiThemePreset::Green);
    assert_eq!(restored.markdown_body_font_family, "Noto Sans");
    assert_eq!(restored.markdown_code_font_family, "Fira Code");
    assert_eq!(restored.markdown_font_size, 16);
    assert!(!restored.notification_enabled);
    assert!(!restored.tab_activity_indicator);
    assert!(!restored.notify_on_process_exit);
    assert!(restored.notify_on_output);
    assert!(!restored.notify_on_bell);
    assert!(!restored.agent_status_notifications);
    assert!(!restored.agent_notify_on_done);
    assert!(!restored.agent_notify_on_blocked);
    assert_eq!(restored.profiles.len(), 1);
    assert_eq!(restored.profiles[0].name, "Dev");
    assert_eq!(restored.profiles[0].shell_path, "/bin/zsh");
    assert_eq!(restored.profiles[0].shell_args, vec!["--login"]);
    assert_eq!(restored.profiles[0].env_vars, "FOO=bar\nBAZ=qux");
    assert_eq!(
        restored.profiles[0].working_directory,
        "/home/user/projects"
    );
    assert!(restored.profiles[0].is_default);
}

#[test]
fn apply_migrations_moves_legacy_font_family_when_primary_unset() {
    let mut s: AppSettings = serde_json::from_str(r#"{"font_family": "Legacy Mono"}"#).unwrap();
    s.apply_migrations();
    assert_eq!(s.font_family_primary, "Legacy Mono");
    assert!(s.font_family.is_empty());
}

#[test]
fn apply_migrations_drops_legacy_font_family_when_primary_set() {
    let mut s: AppSettings = serde_json::from_str(
        r#"{"font_family": "Legacy Mono", "font_family_primary": "New Mono"}"#,
    )
    .unwrap();
    s.apply_migrations();
    assert_eq!(s.font_family_primary, "New Mono");
    assert!(s.font_family.is_empty());
}

// ── shift_enter_behavior (task0003 AC-1) ────────────────────────────

#[test]
fn shift_enter_behavior_defaults_to_alt_enter() {
    assert_eq!(
        AppSettings::default().shift_enter_behavior,
        ShiftEnterBehavior::AltEnter
    );
}

#[test]
fn shift_enter_behavior_each_wire_value_round_trips_through_serde() {
    for (json_value, variant) in [
        ("none", ShiftEnterBehavior::None),
        ("alt_enter", ShiftEnterBehavior::AltEnter),
        ("kitty_csi_u", ShiftEnterBehavior::KittyCsiU),
        ("lf", ShiftEnterBehavior::Lf),
    ] {
        let s: AppSettings =
            serde_json::from_str(&format!(r#"{{"shift_enter_behavior": "{json_value}"}}"#))
                .unwrap();
        assert_eq!(s.shift_enter_behavior, variant);
        let v = serde_json::to_value(&s).unwrap();
        assert_eq!(v["shift_enter_behavior"], json!(json_value));
    }
}

#[test]
fn shift_enter_behavior_null_resolves_to_default() {
    let s: AppSettings = serde_json::from_str(r#"{"shift_enter_behavior": null}"#).unwrap();
    assert_eq!(s.shift_enter_behavior, ShiftEnterBehavior::AltEnter);
}

// ── agent_status_notifications (task0007 AC-5) ──────────────────────

#[test]
fn agent_status_notifications_defaults_to_true() {
    assert!(AppSettings::default().agent_status_notifications);
}

#[test]
fn agent_status_notifications_missing_key_resolves_to_default_true() {
    let s: AppSettings = serde_json::from_str("{}").unwrap();
    assert!(s.agent_status_notifications);
}

#[test]
fn agent_status_notifications_null_resolves_to_default_true() {
    let s: AppSettings = serde_json::from_str(r#"{"agent_status_notifications": null}"#).unwrap();
    assert!(s.agent_status_notifications);
}

#[test]
fn agent_status_notifications_explicit_false_deserializes() {
    let s: AppSettings = serde_json::from_str(r#"{"agent_status_notifications": false}"#).unwrap();
    assert!(!s.agent_status_notifications);
}

// ── agent_notify_on_done / agent_notify_on_blocked (task0001 AC-1) ──

#[test]
fn agent_notify_on_done_defaults_to_true() {
    assert!(AppSettings::default().agent_notify_on_done);
}

#[test]
fn agent_notify_on_done_missing_key_resolves_to_default_true() {
    let s: AppSettings = serde_json::from_str("{}").unwrap();
    assert!(s.agent_notify_on_done);
}

#[test]
fn agent_notify_on_done_null_resolves_to_default_true() {
    let s: AppSettings = serde_json::from_str(r#"{"agent_notify_on_done": null}"#).unwrap();
    assert!(s.agent_notify_on_done);
}

#[test]
fn agent_notify_on_done_explicit_false_deserializes() {
    let s: AppSettings = serde_json::from_str(r#"{"agent_notify_on_done": false}"#).unwrap();
    assert!(!s.agent_notify_on_done);
}

#[test]
fn agent_notify_on_blocked_defaults_to_true() {
    assert!(AppSettings::default().agent_notify_on_blocked);
}

#[test]
fn agent_notify_on_blocked_missing_key_resolves_to_default_true() {
    let s: AppSettings = serde_json::from_str("{}").unwrap();
    assert!(s.agent_notify_on_blocked);
}

#[test]
fn agent_notify_on_blocked_null_resolves_to_default_true() {
    let s: AppSettings = serde_json::from_str(r#"{"agent_notify_on_blocked": null}"#).unwrap();
    assert!(s.agent_notify_on_blocked);
}

#[test]
fn agent_notify_on_blocked_explicit_false_deserializes() {
    let s: AppSettings = serde_json::from_str(r#"{"agent_notify_on_blocked": false}"#).unwrap();
    assert!(!s.agent_notify_on_blocked);
}

#[test]
fn serializing_default_settings_never_emits_legacy_shift_enter_key() {
    let v = serde_json::to_value(AppSettings::default()).unwrap();
    assert!(
        v.get("shift_enter_as_alt_enter").is_none(),
        "legacy shift_enter_as_alt_enter key must not be serialized"
    );
    assert_eq!(v["shift_enter_behavior"], json!("alt_enter"));
}

#[test]
fn apply_migrations_maps_legacy_true_to_alt_enter_when_new_key_absent() {
    let mut s: AppSettings = serde_json::from_str(r#"{"shift_enter_as_alt_enter": true}"#).unwrap();
    assert!(s.apply_migrations());
    assert_eq!(s.shift_enter_behavior, ShiftEnterBehavior::AltEnter);
}

#[test]
fn apply_migrations_maps_legacy_false_to_none_when_new_key_absent() {
    let mut s: AppSettings =
        serde_json::from_str(r#"{"shift_enter_as_alt_enter": false}"#).unwrap();
    assert!(s.apply_migrations());
    assert_eq!(s.shift_enter_behavior, ShiftEnterBehavior::None);
}

#[test]
fn apply_migrations_new_key_present_wins_over_legacy() {
    let mut s: AppSettings = serde_json::from_str(
        r#"{"shift_enter_behavior": "kitty_csi_u", "shift_enter_as_alt_enter": false}"#,
    )
    .unwrap();
    s.apply_migrations();
    assert_eq!(s.shift_enter_behavior, ShiftEnterBehavior::KittyCsiU);
}

/// AC-1 (task0004 rework, rd2-apply-migrations-precedence): the new
/// key wins even when its PRESENT value happens to equal the wire
/// default (`alt_enter`) — this is the case the old "resolved value
/// equals the default means absent" heuristic got wrong, since a
/// present-and-default value is indistinguishable from absent under
/// that heuristic.
#[test]
fn apply_migrations_new_key_present_as_default_value_wins_over_legacy() {
    let mut s: AppSettings = serde_json::from_str(
        r#"{"shift_enter_behavior": "alt_enter", "shift_enter_as_alt_enter": false}"#,
    )
    .unwrap();
    s.apply_migrations();
    assert_eq!(s.shift_enter_behavior, ShiftEnterBehavior::AltEnter);
}

/// AC-2 (task0004 rework): an explicit `null` for the new key counts
/// as PRESENT (it resolves to the wire default), so it still wins
/// over the legacy boolean rather than falling through to it.
#[test]
fn apply_migrations_new_key_present_as_explicit_null_wins_over_legacy() {
    let mut s: AppSettings = serde_json::from_str(
        r#"{"shift_enter_behavior": null, "shift_enter_as_alt_enter": false}"#,
    )
    .unwrap();
    s.apply_migrations();
    assert_eq!(s.shift_enter_behavior, ShiftEnterBehavior::AltEnter);
}

#[test]
fn apply_migrations_no_legacy_key_leaves_default_unmigrated() {
    let mut s = AppSettings::default();
    assert!(!s.apply_migrations());
    assert_eq!(s.shift_enter_behavior, ShiftEnterBehavior::AltEnter);
}

/// Companion to `apply_migrations_no_legacy_key_leaves_default_
/// unmigrated` for the JSON-deserialize path specifically: when
/// BOTH keys are absent from the source JSON, the deserialize-time
/// `Unresolved` sentinel must still resolve to the real wire default
/// (not leak past `apply_migrations`).
#[test]
fn apply_migrations_neither_key_present_resolves_sentinel_to_default() {
    let mut s: AppSettings = serde_json::from_str(r#"{}"#).unwrap();
    assert!(!s.apply_migrations());
    assert_eq!(s.shift_enter_behavior, ShiftEnterBehavior::AltEnter);
    // The sentinel must not survive serialization either.
    let v = serde_json::to_value(&s).unwrap();
    assert_eq!(v["shift_enter_behavior"], json!("alt_enter"));
}
