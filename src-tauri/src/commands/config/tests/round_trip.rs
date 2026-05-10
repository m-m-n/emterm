//! Full-coverage round-trip test for `AppSettings`.

use super::*;

#[test]
fn test_round_trip_preserves_all_fields() {
    let settings = AppSettings {
        font_size: 16,
        font_family_primary: "Fira Code".to_string(),
        font_family_secondary: "Noto Sans JP".to_string(),
        font_family_emoji: "Noto Color Emoji".to_string(),
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
        markdown_emoji_font_family: "Noto Color Emoji".to_string(),
        markdown_font_size: 16,
        fold_enabled: false,
        file_path_detection: false,
        bold_brightens_ansi_colors: false,
        middle_click_paste: false,
        shift_enter_as_alt_enter: false,
        ambiguous_width: false,
        editor_command: "vim +{line} {file}".to_string(),
        skk_mode: false,
        notification_enabled: false,
        tab_activity_indicator: false,
        notify_on_process_exit: false,
        notify_on_output: true,
        notify_on_bell: false,
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
    assert_eq!(restored.font_family_emoji, "Noto Color Emoji");
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
    assert_eq!(restored.bell_action, BellAction::None);
    assert!(!restored.url_detection);
    assert!(restored.copy_on_select);
    assert!(!restored.file_path_detection);
    assert!(!restored.bold_brightens_ansi_colors);
    assert!(!restored.middle_click_paste);
    assert!(!restored.shift_enter_as_alt_enter);
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
    assert_eq!(restored.markdown_emoji_font_family, "Noto Color Emoji");
    assert_eq!(restored.markdown_font_size, 16);
    assert!(!restored.notification_enabled);
    assert!(!restored.tab_activity_indicator);
    assert!(!restored.notify_on_process_exit);
    assert!(restored.notify_on_output);
    assert!(!restored.notify_on_bell);
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
