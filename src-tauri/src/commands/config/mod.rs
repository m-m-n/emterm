pub mod io;
pub mod settings;
pub mod types;
mod validation;

// Re-export main types for external use
pub use io::{load_settings, save_settings};
pub use settings::{AppSettings, KeybindSettings, Profile, SshConnection, SshOption};
pub use types::*;

// ============================================================
// Tests
// ============================================================

#[cfg(test)]
mod tests {
    use super::settings::{AppSettings, KeybindSettings, Profile, SshConnection, SshOption};
    use super::types::*;
    use super::validation::validate_settings;

    // -- Default values --

    #[test]
    fn test_app_settings_default() {
        let settings = AppSettings::default();
        assert_eq!(settings.font_size, 13);
        assert_eq!(settings.font_family_primary, "");
        assert_eq!(settings.font_family_secondary, "");
        assert_eq!(settings.font_family_emoji, "");
        assert_eq!(settings.ui_theme, UiTheme::System);
        assert_eq!(settings.ui_theme_preset, UiThemePreset::Purple);
        assert_eq!(settings.terminal_color_scheme, "");
        assert_eq!(settings.padding, 4);
        assert_eq!(settings.scrollback_lines, 10000);
        assert_eq!(settings.show_scrollbar, ScrollbarMode::Auto);
        assert_eq!(settings.shell_path, "");
        assert!(settings.shell_args.is_empty());
        assert_eq!(settings.cursor_style, CursorStyle::Block);
        assert!(settings.cursor_blink);
        assert_eq!(settings.scroll_speed, 3);
        assert_eq!(settings.bell_action, BellAction::Visual);
        assert!(settings.url_detection);
        assert!(!settings.copy_on_select);
        assert!(settings.file_path_detection);
        assert!(settings.bold_brightens_ansi_colors);
        assert!(settings.middle_click_paste);
        assert!(settings.shift_enter_as_alt_enter);
        assert_eq!(settings.editor_command, "code --goto {file}:{line}:{col}");
        assert!(settings.skk_mode);
        assert_eq!(settings.language, "auto");
        assert_eq!(settings.ui_font_family, "Roboto");
        assert!(settings.show_tab_bar);
        assert!(settings.markdown_theme_follow_ui);
        assert_eq!(settings.markdown_theme, UiTheme::System);
        assert_eq!(settings.markdown_theme_preset, UiThemePreset::Purple);
        assert_eq!(settings.markdown_body_font_family, "");
        assert_eq!(settings.markdown_code_font_family, "");
        assert_eq!(settings.markdown_emoji_font_family, "");
        assert_eq!(settings.markdown_font_size, 14);
        // SSH defaults
        assert_eq!(settings.ssh_command_path, "");
        assert!(settings.ssh_connections.is_empty());
        // Notification defaults
        assert!(settings.notification_enabled);
        assert!(settings.tab_activity_indicator);
        assert!(settings.notify_on_process_exit);
        assert!(!settings.notify_on_output);
        assert!(settings.notify_on_bell);
    }

    #[test]
    fn test_keybind_settings_default() {
        let keybinds = KeybindSettings::default();
        assert_eq!(keybinds.copy, "Ctrl+Shift+C");
        assert_eq!(keybinds.paste, "Ctrl+Shift+V");
        assert_eq!(keybinds.select_all, "Ctrl+Shift+A");
        assert_eq!(keybinds.search, "Ctrl+Shift+F");
        assert_eq!(keybinds.new_tab, "Ctrl+Shift+T");
        assert_eq!(keybinds.close_tab, "Ctrl+Shift+W");
        assert_eq!(keybinds.next_tab, "Ctrl+PageDown");
        assert_eq!(keybinds.prev_tab, "Ctrl+PageUp");
        assert_eq!(keybinds.zoom_in, "Ctrl+Plus");
        assert_eq!(keybinds.zoom_out, "Ctrl+Minus");
        assert_eq!(keybinds.zoom_reset, "Ctrl+0");
        assert_eq!(keybinds.toggle_fullscreen, "F11");
        assert_eq!(keybinds.open_settings, "Ctrl+,");
        assert_eq!(keybinds.toggle_tab_bar, "Ctrl+Shift+B");
    }

    // -- Deserialization --

    #[test]
    fn test_deserialize_empty_json() {
        let json = r#"{}"#;
        let settings: AppSettings = serde_json::from_str(json).unwrap();
        assert_eq!(settings.font_size, 13);
        assert_eq!(settings.ui_theme, UiTheme::System);
        assert_eq!(settings.cursor_style, CursorStyle::Block);
        assert!(settings.cursor_blink);
        assert_eq!(settings.keybinds.copy, "Ctrl+Shift+C");
    }

    #[test]
    fn test_deserialize_old_format() {
        let json = r#"{"font_size": 16}"#;
        let settings: AppSettings = serde_json::from_str(json).unwrap();
        assert_eq!(settings.font_size, 16);
        // All new fields use defaults
        assert_eq!(settings.font_family_primary, "");
        assert_eq!(settings.font_family_secondary, "");
        assert_eq!(settings.font_family_emoji, "");
        assert_eq!(settings.ui_theme, UiTheme::System);
        assert_eq!(settings.cursor_style, CursorStyle::Block);
    }

    #[test]
    fn test_deserialize_null_font_size() {
        let json = r#"{"font_size": null}"#;
        let settings: AppSettings = serde_json::from_str(json).unwrap();
        assert_eq!(settings.font_size, 13);
    }

    #[test]
    fn test_deserialize_null_enum() {
        let json = r#"{"ui_theme": null}"#;
        let settings: AppSettings = serde_json::from_str(json).unwrap();
        assert_eq!(settings.ui_theme, UiTheme::System);
    }

    #[test]
    fn test_deserialize_null_keybind() {
        let json = r#"{"keybinds": {"copy": null}}"#;
        let settings: AppSettings = serde_json::from_str(json).unwrap();
        // null keybind falls back to custom default via deserialize_null_keybind_copy
        assert_eq!(settings.keybinds.copy, "Ctrl+Shift+C");
        // Non-null keybinds still use serde(default) function
        assert_eq!(settings.keybinds.paste, "Ctrl+Shift+V");
    }

    #[test]
    fn test_deserialize_null_all_custom_defaults() {
        let json = r#"{
            "font_size": null,
            "line_height": null,
            "padding": null,
            "scrollback_lines": null,
            "scroll_speed": null,
            "cursor_blink": null,
            "url_detection": null
        }"#;
        let settings: AppSettings = serde_json::from_str(json).unwrap();
        assert_eq!(settings.font_size, 13);
        assert_eq!(settings.padding, 4);
        assert_eq!(settings.scrollback_lines, 10000);
        assert_eq!(settings.scroll_speed, 3);
        assert!(settings.cursor_blink);
        assert!(settings.url_detection);
    }

    #[test]
    fn test_deserialize_ignores_unknown_fields() {
        let json = r#"{"font_size": 14, "unknown_field": "value"}"#;
        // serde by default ignores unknown fields (no deny_unknown_fields)
        let settings: AppSettings = serde_json::from_str(json).unwrap();
        assert_eq!(settings.font_size, 14);
    }

    #[test]
    fn test_deserialize_invalid_enum_errors() {
        let json = r#"{"ui_theme": "invalid"}"#;
        let result = serde_json::from_str::<AppSettings>(json);
        assert!(result.is_err());
    }

    #[test]
    fn test_deserialize_invalid_cursor_style_errors() {
        let json = r#"{"cursor_style": "invalid"}"#;
        let result = serde_json::from_str::<AppSettings>(json);
        assert!(result.is_err());
    }

    #[test]
    fn test_deserialize_invalid_bell_action_errors() {
        let json = r#"{"bell_action": "invalid"}"#;
        let result = serde_json::from_str::<AppSettings>(json);
        assert!(result.is_err());
    }

    #[test]
    fn test_deserialize_invalid_scrollbar_mode_errors() {
        let json = r#"{"show_scrollbar": "invalid"}"#;
        let result = serde_json::from_str::<AppSettings>(json);
        assert!(result.is_err());
    }

    // -- UiThemePreset --

    #[test]
    fn test_ui_theme_preset_default_is_purple() {
        assert_eq!(UiThemePreset::default(), UiThemePreset::Purple);
    }

    #[test]
    fn test_deserialize_ui_theme_preset_values() {
        let test_cases = vec![
            (r#""purple""#, UiThemePreset::Purple),
            (r#""blue""#, UiThemePreset::Blue),
            (r#""green""#, UiThemePreset::Green),
            (r#""orange""#, UiThemePreset::Orange),
            (r#""pink""#, UiThemePreset::Pink),
        ];
        for (json, expected) in test_cases {
            let result: UiThemePreset = serde_json::from_str(json).unwrap();
            assert_eq!(result, expected, "Failed for {}", json);
        }
    }

    #[test]
    fn test_deserialize_null_ui_theme_preset() {
        let json = r#"{"ui_theme_preset": null}"#;
        let settings: AppSettings = serde_json::from_str(json).unwrap();
        assert_eq!(settings.ui_theme_preset, UiThemePreset::Purple);
    }

    #[test]
    fn test_deserialize_missing_ui_theme_preset() {
        let json = r#"{}"#;
        let settings: AppSettings = serde_json::from_str(json).unwrap();
        assert_eq!(settings.ui_theme_preset, UiThemePreset::Purple);
    }

    #[test]
    fn test_deserialize_invalid_ui_theme_preset_errors() {
        let json = r#"{"ui_theme_preset": "invalid"}"#;
        let result = serde_json::from_str::<AppSettings>(json);
        assert!(result.is_err());
    }

    #[test]
    fn test_ui_theme_preset_round_trip() {
        let test_cases = vec![
            UiThemePreset::Purple,
            UiThemePreset::Blue,
            UiThemePreset::Green,
            UiThemePreset::Orange,
            UiThemePreset::Pink,
        ];
        for preset in test_cases {
            let json = serde_json::to_string(&preset).unwrap();
            let restored: UiThemePreset = serde_json::from_str(&json).unwrap();
            assert_eq!(restored, preset);
        }
    }

    // -- Serialization --

    #[test]
    fn test_serialize_enums_lowercase() {
        let settings = AppSettings::default();
        let json = serde_json::to_string(&settings).unwrap();
        assert!(json.contains("\"ui_theme\":\"system\""));
        assert!(json.contains("\"ui_theme_preset\":\"purple\""));
        assert!(json.contains("\"cursor_style\":\"block\""));
        assert!(json.contains("\"bell_action\":\"visual\""));
        assert!(json.contains("\"show_scrollbar\":\"auto\""));
        assert!(json.contains("\"markdown_theme\":\"system\""));
        assert!(json.contains("\"markdown_theme_preset\":\"purple\""));
        assert!(json.contains("\"markdown_theme_follow_ui\":true"));
    }

    // -- Round-trip --

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

    #[test]
    fn test_shell_args_round_trip() {
        let json = r#"{"shell_args": ["--login", "-i"]}"#;
        let settings: AppSettings = serde_json::from_str(json).unwrap();
        assert_eq!(settings.shell_args, vec!["--login", "-i"]);

        let serialized = serde_json::to_string(&settings).unwrap();
        let restored: AppSettings = serde_json::from_str(&serialized).unwrap();
        assert_eq!(restored.shell_args, vec!["--login", "-i"]);
    }

    // -- Validation --

    // -- Language field deserialization --

    #[test]
    fn test_deserialize_missing_language_defaults_to_auto() {
        let json = r#"{}"#;
        let settings: AppSettings = serde_json::from_str(json).unwrap();
        assert_eq!(settings.language, "auto");
    }

    #[test]
    fn test_deserialize_null_language_defaults_to_auto() {
        let json = r#"{"language": null}"#;
        let settings: AppSettings = serde_json::from_str(json).unwrap();
        assert_eq!(settings.language, "auto");
    }

    #[test]
    fn test_deserialize_language_ja() {
        let json = r#"{"language": "ja"}"#;
        let settings: AppSettings = serde_json::from_str(json).unwrap();
        assert_eq!(settings.language, "ja");
    }

    #[test]
    fn test_deserialize_language_en() {
        let json = r#"{"language": "en"}"#;
        let settings: AppSettings = serde_json::from_str(json).unwrap();
        assert_eq!(settings.language, "en");
    }

    #[test]
    fn test_language_round_trip() {
        let mut settings = AppSettings::default();
        settings.language = "ja".to_string();
        let json = serde_json::to_string(&settings).unwrap();
        let restored: AppSettings = serde_json::from_str(&json).unwrap();
        assert_eq!(restored.language, "ja");
    }

    // -- UI font family tests --

    #[test]
    fn test_deserialize_missing_ui_font_family_defaults_to_roboto() {
        let json = r#"{}"#;
        let settings: AppSettings = serde_json::from_str(json).unwrap();
        assert_eq!(settings.ui_font_family, "Roboto");
    }

    #[test]
    fn test_deserialize_null_ui_font_family_defaults_to_roboto() {
        let json = r#"{"ui_font_family": null}"#;
        let settings: AppSettings = serde_json::from_str(json).unwrap();
        assert_eq!(settings.ui_font_family, "Roboto");
    }

    #[test]
    fn test_deserialize_ui_font_family_custom_value() {
        let json = r#"{"ui_font_family": "Noto Sans"}"#;
        let settings: AppSettings = serde_json::from_str(json).unwrap();
        assert_eq!(settings.ui_font_family, "Noto Sans");
    }

    #[test]
    fn test_ui_font_family_round_trip() {
        let mut settings = AppSettings::default();
        settings.ui_font_family = "Open Sans".to_string();
        let json = serde_json::to_string(&settings).unwrap();
        let restored: AppSettings = serde_json::from_str(&json).unwrap();
        assert_eq!(restored.ui_font_family, "Open Sans");
    }

    #[test]
    fn test_validate_valid_settings() {
        let settings = AppSettings::default();
        assert!(validate_settings(&settings).is_ok());
    }

    #[test]
    fn test_validate_rejects_font_size_below_min() {
        let mut settings = AppSettings::default();
        settings.font_size = 7;
        assert!(validate_settings(&settings).is_err());
    }

    #[test]
    fn test_validate_rejects_font_size_above_max() {
        let mut settings = AppSettings::default();
        settings.font_size = 33;
        assert!(validate_settings(&settings).is_err());
    }

    #[test]
    fn test_validate_rejects_scroll_speed_below_min() {
        let mut settings = AppSettings::default();
        settings.scroll_speed = 0;
        assert!(validate_settings(&settings).is_err());
    }

    #[test]
    fn test_validate_rejects_scroll_speed_above_max() {
        let mut settings = AppSettings::default();
        settings.scroll_speed = 11;
        assert!(validate_settings(&settings).is_err());
    }

    #[test]
    fn test_validate_rejects_padding_above_max() {
        let mut settings = AppSettings::default();
        settings.padding = 33;
        assert!(validate_settings(&settings).is_err());
    }

    #[test]
    fn test_validate_rejects_scrollback_above_max() {
        let mut settings = AppSettings::default();
        settings.scrollback_lines = 100001;
        assert!(validate_settings(&settings).is_err());
    }

    #[test]
    fn test_validate_accepts_boundary_values() {
        let mut settings = AppSettings::default();
        settings.font_size = MIN_FONT_SIZE;
        assert!(validate_settings(&settings).is_ok());

        settings.font_size = MAX_FONT_SIZE;
        assert!(validate_settings(&settings).is_ok());

        settings.scroll_speed = MIN_SCROLL_SPEED;
        assert!(validate_settings(&settings).is_ok());

        settings.scroll_speed = MAX_SCROLL_SPEED;
        assert!(validate_settings(&settings).is_ok());
    }

    // -- Font family migration --

    #[test]
    fn test_migrate_legacy_font_family_to_primary() {
        let json = r#"{"font_family": "Fira Code"}"#;
        let mut settings: AppSettings = serde_json::from_str(json).unwrap();
        // Simulate migration (load_settings does this)
        if !settings.font_family.is_empty() && settings.font_family_primary.is_empty() {
            settings.font_family_primary = std::mem::take(&mut settings.font_family);
        } else {
            settings.font_family.clear();
        }
        assert_eq!(settings.font_family_primary, "Fira Code");
        assert_eq!(settings.font_family_secondary, "");
        assert_eq!(settings.font_family_emoji, "");
    }

    #[test]
    fn test_migrate_font_family_primary_takes_precedence() {
        let json = r#"{"font_family": "Old Font", "font_family_primary": "New Font"}"#;
        let mut settings: AppSettings = serde_json::from_str(json).unwrap();
        if !settings.font_family.is_empty() && settings.font_family_primary.is_empty() {
            settings.font_family_primary = std::mem::take(&mut settings.font_family);
        } else {
            settings.font_family.clear();
        }
        assert_eq!(settings.font_family_primary, "New Font");
    }

    #[test]
    fn test_font_family_not_serialized() {
        let settings = AppSettings::default();
        let json = serde_json::to_string(&settings).unwrap();
        assert!(!json.contains("\"font_family\""));
        assert!(json.contains("\"font_family_primary\""));
        assert!(json.contains("\"font_family_secondary\""));
        assert!(json.contains("\"font_family_emoji\""));
    }

    #[test]
    fn test_deserialize_null_font_family_fields() {
        let json = r#"{"font_family_primary": null, "font_family_secondary": null, "font_family_emoji": null}"#;
        let settings: AppSettings = serde_json::from_str(json).unwrap();
        assert_eq!(settings.font_family_primary, "");
        assert_eq!(settings.font_family_secondary, "");
        assert_eq!(settings.font_family_emoji, "");
    }

    #[test]
    fn test_three_font_family_fields_round_trip() {
        let mut settings = AppSettings::default();
        settings.font_family_primary = "JetBrains Mono".to_string();
        settings.font_family_secondary = "Noto Sans JP".to_string();
        settings.font_family_emoji = "Noto Color Emoji".to_string();
        let json = serde_json::to_string(&settings).unwrap();
        let restored: AppSettings = serde_json::from_str(&json).unwrap();
        assert_eq!(restored.font_family_primary, "JetBrains Mono");
        assert_eq!(restored.font_family_secondary, "Noto Sans JP");
        assert_eq!(restored.font_family_emoji, "Noto Color Emoji");
    }

    // -- show_tab_bar tests --

    #[test]
    fn test_deserialize_missing_show_tab_bar_defaults_to_true() {
        let json = r#"{}"#;
        let settings: AppSettings = serde_json::from_str(json).unwrap();
        assert!(settings.show_tab_bar);
    }

    #[test]
    fn test_deserialize_null_show_tab_bar_defaults_to_true() {
        let json = r#"{"show_tab_bar": null}"#;
        let settings: AppSettings = serde_json::from_str(json).unwrap();
        assert!(settings.show_tab_bar);
    }

    #[test]
    fn test_show_tab_bar_false_round_trip() {
        let mut settings = AppSettings::default();
        settings.show_tab_bar = false;
        let json = serde_json::to_string(&settings).unwrap();
        let restored: AppSettings = serde_json::from_str(&json).unwrap();
        assert!(!restored.show_tab_bar);
    }

    // -- toggle_tab_bar keybind tests --

    #[test]
    fn test_deserialize_missing_toggle_tab_bar_keybind_defaults() {
        let json = r#"{"keybinds": {}}"#;
        let settings: AppSettings = serde_json::from_str(json).unwrap();
        assert_eq!(settings.keybinds.toggle_tab_bar, "Ctrl+Shift+B");
    }

    #[test]
    fn test_deserialize_null_toggle_tab_bar_keybind_defaults() {
        let json = r#"{"keybinds": {"toggle_tab_bar": null}}"#;
        let settings: AppSettings = serde_json::from_str(json).unwrap();
        assert_eq!(settings.keybinds.toggle_tab_bar, "Ctrl+Shift+B");
    }

    #[test]
    fn test_toggle_tab_bar_keybind_custom_value() {
        let json = r#"{"keybinds": {"toggle_tab_bar": "Ctrl+Shift+H"}}"#;
        let settings: AppSettings = serde_json::from_str(json).unwrap();
        assert_eq!(settings.keybinds.toggle_tab_bar, "Ctrl+Shift+H");
    }

    // -- UserColorScheme tests --

    #[test]
    fn test_deserialize_missing_custom_color_schemes_defaults_to_empty() {
        let json = r#"{}"#;
        let settings: AppSettings = serde_json::from_str(json).unwrap();
        assert!(settings.custom_color_schemes.is_empty());
    }

    #[test]
    fn test_deserialize_null_custom_color_schemes_defaults_to_empty() {
        let json = r#"{"custom_color_schemes": null}"#;
        let settings: AppSettings = serde_json::from_str(json).unwrap();
        assert!(settings.custom_color_schemes.is_empty());
    }

    #[test]
    fn test_user_color_scheme_round_trip() {
        let scheme = UserColorScheme {
            name: "my_theme".to_string(),
            foreground: "#f8f8f2".to_string(),
            background: "#282a36".to_string(),
            cursor: "#f8f8f2".to_string(),
            selection: "#44475a".to_string(),
            ansi_colors: vec![
                "#21222c".to_string(),
                "#ff5555".to_string(),
                "#50fa7b".to_string(),
                "#f1fa8c".to_string(),
                "#bd93f9".to_string(),
                "#ff79c6".to_string(),
                "#8be9fd".to_string(),
                "#f8f8f2".to_string(),
                "#6272a4".to_string(),
                "#ff6e6e".to_string(),
                "#69ff94".to_string(),
                "#ffffa5".to_string(),
                "#d6acff".to_string(),
                "#ff92df".to_string(),
                "#a4ffff".to_string(),
                "#ffffff".to_string(),
            ],
        };

        let json = serde_json::to_string(&scheme).unwrap();
        let restored: UserColorScheme = serde_json::from_str(&json).unwrap();
        assert_eq!(restored, scheme);
    }

    #[test]
    fn test_settings_with_custom_color_schemes_round_trip() {
        let mut settings = AppSettings::default();
        settings.custom_color_schemes = vec![
            UserColorScheme {
                name: "theme1".to_string(),
                foreground: "#ffffff".to_string(),
                background: "#000000".to_string(),
                cursor: "#ffffff".to_string(),
                selection: "#333333".to_string(),
                ansi_colors: (0..16)
                    .map(|i| format!("#{:02x}{:02x}{:02x}", i * 16, i * 16, i * 16))
                    .collect(),
            },
            UserColorScheme {
                name: "theme2".to_string(),
                foreground: "#00ff00".to_string(),
                background: "#001100".to_string(),
                cursor: "#00ff00".to_string(),
                selection: "#003300".to_string(),
                ansi_colors: (0..16).map(|i| format!("#00{:02x}00", i * 16)).collect(),
            },
        ];

        let json = serde_json::to_string(&settings).unwrap();
        let restored: AppSettings = serde_json::from_str(&json).unwrap();
        assert_eq!(restored.custom_color_schemes.len(), 2);
        assert_eq!(restored.custom_color_schemes[0].name, "theme1");
        assert_eq!(restored.custom_color_schemes[1].name, "theme2");
    }

    #[test]
    fn test_app_settings_default_has_empty_custom_color_schemes() {
        let settings = AppSettings::default();
        assert!(settings.custom_color_schemes.is_empty());
    }

    // -- Markdown Viewer settings tests --

    #[test]
    fn test_markdown_settings_defaults() {
        let settings = AppSettings::default();
        assert_eq!(settings.markdown_body_font_family, "");
        assert_eq!(settings.markdown_code_font_family, "");
        assert_eq!(settings.markdown_emoji_font_family, "");
        assert_eq!(settings.markdown_font_size, 14);
    }

    #[test]
    fn test_deserialize_missing_markdown_fields_use_defaults() {
        let json = r#"{}"#;
        let settings: AppSettings = serde_json::from_str(json).unwrap();
        assert_eq!(settings.markdown_body_font_family, "");
        assert_eq!(settings.markdown_code_font_family, "");
        assert_eq!(settings.markdown_emoji_font_family, "");
        assert_eq!(settings.markdown_font_size, 14);
    }

    #[test]
    fn test_deserialize_null_markdown_fields_use_defaults() {
        let json = r#"{
            "markdown_body_font_family": null,
            "markdown_code_font_family": null,
            "markdown_emoji_font_family": null,
            "markdown_font_size": null
        }"#;
        let settings: AppSettings = serde_json::from_str(json).unwrap();
        assert_eq!(settings.markdown_body_font_family, "");
        assert_eq!(settings.markdown_code_font_family, "");
        assert_eq!(settings.markdown_emoji_font_family, "");
        assert_eq!(settings.markdown_font_size, 14);
    }

    #[test]
    fn test_deserialize_markdown_fields_explicit_values() {
        let json = r#"{
            "markdown_body_font_family": "Noto Sans",
            "markdown_code_font_family": "Fira Code",
            "markdown_emoji_font_family": "Noto Color Emoji",
            "markdown_font_size": 18
        }"#;
        let settings: AppSettings = serde_json::from_str(json).unwrap();
        assert_eq!(settings.markdown_body_font_family, "Noto Sans");
        assert_eq!(settings.markdown_code_font_family, "Fira Code");
        assert_eq!(settings.markdown_emoji_font_family, "Noto Color Emoji");
        assert_eq!(settings.markdown_font_size, 18);
    }

    #[test]
    fn test_validate_markdown_font_size_below_min() {
        let mut settings = AppSettings::default();
        settings.markdown_font_size = 7;
        assert!(validate_settings(&settings).is_err());
    }

    #[test]
    fn test_validate_markdown_font_size_above_max() {
        let mut settings = AppSettings::default();
        settings.markdown_font_size = 33;
        assert!(validate_settings(&settings).is_err());
    }

    #[test]
    fn test_validate_markdown_font_size_min_boundary() {
        let mut settings = AppSettings::default();
        settings.markdown_font_size = MIN_FONT_SIZE;
        assert!(validate_settings(&settings).is_ok());
    }

    #[test]
    fn test_validate_markdown_font_size_max_boundary() {
        let mut settings = AppSettings::default();
        settings.markdown_font_size = MAX_FONT_SIZE;
        assert!(validate_settings(&settings).is_ok());
    }

    // -- Markdown Theme settings tests --

    #[test]
    fn test_markdown_theme_follow_ui_default_is_true() {
        let settings = AppSettings::default();
        assert!(settings.markdown_theme_follow_ui);
    }

    #[test]
    fn test_markdown_theme_default_is_system() {
        let settings = AppSettings::default();
        assert_eq!(settings.markdown_theme, UiTheme::System);
    }

    #[test]
    fn test_markdown_theme_preset_default_is_purple() {
        let settings = AppSettings::default();
        assert_eq!(settings.markdown_theme_preset, UiThemePreset::Purple);
    }

    #[test]
    fn test_deserialize_missing_markdown_theme_fields_use_defaults() {
        let json = r#"{}"#;
        let settings: AppSettings = serde_json::from_str(json).unwrap();
        assert!(settings.markdown_theme_follow_ui);
        assert_eq!(settings.markdown_theme, UiTheme::System);
        assert_eq!(settings.markdown_theme_preset, UiThemePreset::Purple);
    }

    #[test]
    fn test_deserialize_null_markdown_theme_fields_use_defaults() {
        let json = r#"{
            "markdown_theme_follow_ui": null,
            "markdown_theme": null,
            "markdown_theme_preset": null
        }"#;
        let settings: AppSettings = serde_json::from_str(json).unwrap();
        assert!(settings.markdown_theme_follow_ui);
        assert_eq!(settings.markdown_theme, UiTheme::System);
        assert_eq!(settings.markdown_theme_preset, UiThemePreset::Purple);
    }

    #[test]
    fn test_markdown_theme_fields_round_trip() {
        let mut settings = AppSettings::default();
        settings.markdown_theme_follow_ui = false;
        settings.markdown_theme = UiTheme::Dark;
        settings.markdown_theme_preset = UiThemePreset::Orange;
        let json = serde_json::to_string(&settings).unwrap();
        let restored: AppSettings = serde_json::from_str(&json).unwrap();
        assert!(!restored.markdown_theme_follow_ui);
        assert_eq!(restored.markdown_theme, UiTheme::Dark);
        assert_eq!(restored.markdown_theme_preset, UiThemePreset::Orange);
    }

    #[test]
    fn test_deserialize_invalid_markdown_theme_errors() {
        let json = r#"{"markdown_theme": "invalid"}"#;
        let result = serde_json::from_str::<AppSettings>(json);
        assert!(result.is_err());
    }

    #[test]
    fn test_deserialize_invalid_markdown_theme_preset_errors() {
        let json = r#"{"markdown_theme_preset": "invalid"}"#;
        let result = serde_json::from_str::<AppSettings>(json);
        assert!(result.is_err());
    }

    #[test]
    fn test_markdown_settings_round_trip() {
        let mut settings = AppSettings::default();
        settings.markdown_body_font_family = "Georgia".to_string();
        settings.markdown_code_font_family = "JetBrains Mono".to_string();
        settings.markdown_font_size = 20;
        let json = serde_json::to_string(&settings).unwrap();
        let restored: AppSettings = serde_json::from_str(&json).unwrap();
        assert_eq!(restored.markdown_body_font_family, "Georgia");
        assert_eq!(restored.markdown_code_font_family, "JetBrains Mono");
        assert_eq!(restored.markdown_font_size, 20);
    }

    // -- Profile tests --

    #[test]
    fn test_app_settings_default_has_empty_profiles() {
        let settings = AppSettings::default();
        assert!(settings.profiles.is_empty());
    }

    #[test]
    fn test_deserialize_missing_profiles_defaults_to_empty() {
        let json = r#"{}"#;
        let settings: AppSettings = serde_json::from_str(json).unwrap();
        assert!(settings.profiles.is_empty());
    }

    #[test]
    fn test_deserialize_null_profiles_defaults_to_empty() {
        let json = r#"{"profiles": null}"#;
        let settings: AppSettings = serde_json::from_str(json).unwrap();
        assert!(settings.profiles.is_empty());
    }

    #[test]
    fn test_profile_round_trip() {
        let profile = Profile {
            name: "My Shell".to_string(),
            shell_path: "/usr/bin/fish".to_string(),
            shell_args: vec!["-l".to_string()],
            env_vars: "TERM=xterm-256color".to_string(),
            working_directory: "/tmp".to_string(),
            is_default: false,
            ssh_connection_name: String::new(),
            wsl_distro_name: String::new(),
        };
        let json = serde_json::to_string(&profile).unwrap();
        let restored: Profile = serde_json::from_str(&json).unwrap();
        assert_eq!(restored, profile);
    }

    #[test]
    fn test_profile_null_fields_use_defaults() {
        let json = r#"{
            "name": "Test",
            "shell_path": null,
            "shell_args": null,
            "env_vars": null,
            "working_directory": null,
            "is_default": null
        }"#;
        let profile: Profile = serde_json::from_str(json).unwrap();
        assert_eq!(profile.name, "Test");
        assert_eq!(profile.shell_path, "");
        assert!(profile.shell_args.is_empty());
        assert_eq!(profile.env_vars, "");
        assert_eq!(profile.working_directory, "");
        assert!(!profile.is_default);
    }

    #[test]
    fn test_profile_missing_optional_fields_use_defaults() {
        let json = r#"{"name": "Minimal"}"#;
        let profile: Profile = serde_json::from_str(json).unwrap();
        assert_eq!(profile.name, "Minimal");
        assert_eq!(profile.shell_path, "");
        assert!(profile.shell_args.is_empty());
        assert_eq!(profile.env_vars, "");
        assert_eq!(profile.working_directory, "");
        assert!(!profile.is_default);
    }

    #[test]
    fn test_settings_with_profiles_round_trip() {
        let mut settings = AppSettings::default();
        settings.profiles = vec![
            Profile {
                name: "Default".to_string(),
                shell_path: "/bin/bash".to_string(),
                shell_args: vec![],
                env_vars: String::new(),
                working_directory: String::new(),
                is_default: true,
                ssh_connection_name: String::new(),
                wsl_distro_name: String::new(),
            },
            Profile {
                name: "Dev".to_string(),
                shell_path: "/bin/zsh".to_string(),
                shell_args: vec!["--login".to_string()],
                env_vars: "NODE_ENV=development".to_string(),
                working_directory: "/home/user/dev".to_string(),
                is_default: false,
                ssh_connection_name: String::new(),
                wsl_distro_name: String::new(),
            },
        ];
        let json = serde_json::to_string(&settings).unwrap();
        let restored: AppSettings = serde_json::from_str(&json).unwrap();
        assert_eq!(restored.profiles.len(), 2);
        assert_eq!(restored.profiles[0].name, "Default");
        assert!(restored.profiles[0].is_default);
        assert_eq!(restored.profiles[1].name, "Dev");
        assert_eq!(restored.profiles[1].shell_path, "/bin/zsh");
    }

    #[test]
    fn test_validate_rejects_empty_profile_name() {
        let mut settings = AppSettings::default();
        settings.profiles = vec![Profile {
            name: "".to_string(),
            shell_path: String::new(),
            shell_args: vec![],
            env_vars: String::new(),
            working_directory: String::new(),
            is_default: false,
            ssh_connection_name: String::new(),
            wsl_distro_name: String::new(),
        }];
        assert!(validate_settings(&settings).is_err());
    }

    #[test]
    fn test_validate_rejects_whitespace_only_profile_name() {
        let mut settings = AppSettings::default();
        settings.profiles = vec![Profile {
            name: "   ".to_string(),
            shell_path: String::new(),
            shell_args: vec![],
            env_vars: String::new(),
            working_directory: String::new(),
            is_default: false,
            ssh_connection_name: String::new(),
            wsl_distro_name: String::new(),
        }];
        assert!(validate_settings(&settings).is_err());
    }

    #[test]
    fn test_validate_accepts_valid_profiles() {
        let mut settings = AppSettings::default();
        settings.profiles = vec![
            Profile {
                name: "Shell 1".to_string(),
                shell_path: String::new(),
                shell_args: vec![],
                env_vars: String::new(),
                working_directory: String::new(),
                is_default: true,
                ssh_connection_name: String::new(),
                wsl_distro_name: String::new(),
            },
            Profile {
                name: "Shell 2".to_string(),
                shell_path: "/bin/fish".to_string(),
                shell_args: vec![],
                env_vars: String::new(),
                working_directory: String::new(),
                is_default: false,
                ssh_connection_name: String::new(),
                wsl_distro_name: String::new(),
            },
        ];
        assert!(validate_settings(&settings).is_ok());
    }

    // -- Notification settings tests --

    #[test]
    fn test_deserialize_missing_notification_fields_use_defaults() {
        let json = r#"{}"#;
        let settings: AppSettings = serde_json::from_str(json).unwrap();
        assert!(settings.notification_enabled);
        assert!(settings.tab_activity_indicator);
        assert!(settings.notify_on_process_exit);
        assert!(!settings.notify_on_output);
        assert!(settings.notify_on_bell);
    }

    #[test]
    fn test_deserialize_null_notification_fields_use_defaults() {
        let json = r#"{
            "notification_enabled": null,
            "tab_activity_indicator": null,
            "notify_on_process_exit": null,
            "notify_on_output": null,
            "notify_on_bell": null
        }"#;
        let settings: AppSettings = serde_json::from_str(json).unwrap();
        assert!(settings.notification_enabled);
        assert!(settings.tab_activity_indicator);
        assert!(settings.notify_on_process_exit);
        assert!(!settings.notify_on_output);
        assert!(settings.notify_on_bell);
    }

    #[test]
    fn test_notification_settings_round_trip() {
        let mut settings = AppSettings::default();
        settings.notification_enabled = false;
        settings.tab_activity_indicator = false;
        settings.notify_on_process_exit = false;
        settings.notify_on_output = true;
        settings.notify_on_bell = false;
        let json = serde_json::to_string(&settings).unwrap();
        let restored: AppSettings = serde_json::from_str(&json).unwrap();
        assert!(!restored.notification_enabled);
        assert!(!restored.tab_activity_indicator);
        assert!(!restored.notify_on_process_exit);
        assert!(restored.notify_on_output);
        assert!(!restored.notify_on_bell);
    }

    // -- SSH Connection tests --

    #[test]
    fn test_app_settings_default_has_empty_ssh_fields() {
        let settings = AppSettings::default();
        assert_eq!(settings.ssh_command_path, "");
        assert!(settings.ssh_connections.is_empty());
    }

    #[test]
    fn test_deserialize_missing_ssh_fields_use_defaults() {
        let json = r#"{}"#;
        let settings: AppSettings = serde_json::from_str(json).unwrap();
        assert_eq!(settings.ssh_command_path, "");
        assert!(settings.ssh_connections.is_empty());
    }

    #[test]
    fn test_deserialize_null_ssh_fields_use_defaults() {
        let json = r#"{"ssh_command_path": null, "ssh_connections": null}"#;
        let settings: AppSettings = serde_json::from_str(json).unwrap();
        assert_eq!(settings.ssh_command_path, "");
        assert!(settings.ssh_connections.is_empty());
    }

    #[test]
    fn test_ssh_connection_serialization() {
        let conn = SshConnection {
            name: "My Server".to_string(),
            hostname: "example.com".to_string(),
            port: 22,
            username: "admin".to_string(),
            identity_file: "~/.ssh/id_rsa".to_string(),
            ssh_options: vec![SshOption {
                key: "StrictHostKeyChecking".to_string(),
                value: "no".to_string(),
            }],
            extra_options: String::new(),
        };
        let json = serde_json::to_string(&conn).unwrap();
        let restored: SshConnection = serde_json::from_str(&json).unwrap();
        assert_eq!(restored.name, conn.name);
        assert_eq!(restored.ssh_options.len(), 1);
        assert_eq!(restored.ssh_options[0].key, "StrictHostKeyChecking");
    }

    #[test]
    fn test_ssh_connection_defaults() {
        let json = r#"{"name": "test", "hostname": "host.com"}"#;
        let conn: SshConnection = serde_json::from_str(json).unwrap();
        assert_eq!(conn.name, "test");
        assert_eq!(conn.hostname, "host.com");
        assert_eq!(conn.port, 22);
        assert_eq!(conn.username, "");
        assert_eq!(conn.identity_file, "");
        assert!(conn.ssh_options.is_empty());
    }

    #[test]
    fn test_ssh_connection_backward_compat_extra_options() {
        // Old settings.json with extra_options should still deserialize
        let json = r#"{"name": "old", "hostname": "host.com", "extra_options": "-o Foo=bar"}"#;
        let conn: SshConnection = serde_json::from_str(json).unwrap();
        assert_eq!(conn.extra_options, "-o Foo=bar");
        assert!(conn.ssh_options.is_empty());
    }

    #[test]
    fn test_ssh_connection_null_port_defaults_to_22() {
        let json = r#"{"name": "test", "hostname": "host.com", "port": null}"#;
        let conn: SshConnection = serde_json::from_str(json).unwrap();
        assert_eq!(conn.port, 22);
    }

    #[test]
    fn test_settings_with_ssh_connections_round_trip() {
        let mut settings = AppSettings::default();
        settings.ssh_command_path = "/usr/bin/ssh".to_string();
        settings.ssh_connections = vec![
            SshConnection {
                name: "Server 1".to_string(),
                hostname: "server1.example.com".to_string(),
                port: 22,
                username: "user".to_string(),
                identity_file: String::new(),
                ssh_options: Vec::new(),
                extra_options: String::new(),
            },
            SshConnection {
                name: "Server 2".to_string(),
                hostname: "server2.example.com".to_string(),
                port: 2222,
                username: String::new(),
                identity_file: "~/.ssh/id_ed25519".to_string(),
                ssh_options: vec![SshOption {
                    key: "StrictHostKeyChecking".to_string(),
                    value: "no".to_string(),
                }],
                extra_options: String::new(),
            },
        ];
        let json = serde_json::to_string(&settings).unwrap();
        let restored: AppSettings = serde_json::from_str(&json).unwrap();
        assert_eq!(restored.ssh_command_path, "/usr/bin/ssh");
        assert_eq!(restored.ssh_connections.len(), 2);
        assert_eq!(restored.ssh_connections[0].name, "Server 1");
        assert_eq!(restored.ssh_connections[1].port, 2222);
        assert_eq!(restored.ssh_connections[1].ssh_options.len(), 1);
    }

    #[test]
    fn test_profile_ssh_connection_name_default() {
        let json = r#"{"name": "Test"}"#;
        let profile: Profile = serde_json::from_str(json).unwrap();
        assert_eq!(profile.ssh_connection_name, "");
    }

    #[test]
    fn test_profile_ssh_connection_name_round_trip() {
        let profile = Profile {
            name: "SSH Profile".to_string(),
            shell_path: String::new(),
            shell_args: vec![],
            env_vars: String::new(),
            working_directory: String::new(),
            is_default: false,
            ssh_connection_name: "My Server".to_string(),
            wsl_distro_name: String::new(),
        };
        let json = serde_json::to_string(&profile).unwrap();
        let restored: Profile = serde_json::from_str(&json).unwrap();
        assert_eq!(restored.ssh_connection_name, "My Server");
    }

    fn make_ssh_conn(name: &str, hostname: &str, port: u16) -> SshConnection {
        SshConnection {
            name: name.to_string(),
            hostname: hostname.to_string(),
            port,
            username: String::new(),
            identity_file: String::new(),
            ssh_options: Vec::new(),
            extra_options: String::new(),
        }
    }

    #[test]
    fn test_validate_rejects_empty_ssh_connection_name() {
        let mut settings = AppSettings::default();
        settings.ssh_connections = vec![make_ssh_conn("", "host.com", 22)];
        assert!(validate_settings(&settings).is_err());
    }

    #[test]
    fn test_validate_rejects_empty_ssh_hostname() {
        let mut settings = AppSettings::default();
        settings.ssh_connections = vec![make_ssh_conn("Test", "", 22)];
        assert!(validate_settings(&settings).is_err());
    }

    #[test]
    fn test_validate_rejects_port_zero() {
        let mut settings = AppSettings::default();
        settings.ssh_connections = vec![make_ssh_conn("Test", "host.com", 0)];
        assert!(validate_settings(&settings).is_err());
    }

    #[test]
    fn test_validate_accepts_port_1() {
        let mut settings = AppSettings::default();
        settings.ssh_connections = vec![make_ssh_conn("Test", "host.com", 1)];
        assert!(validate_settings(&settings).is_ok());
    }

    #[test]
    fn test_validate_accepts_port_65535() {
        let mut settings = AppSettings::default();
        settings.ssh_connections = vec![make_ssh_conn("Test", "host.com", 65535)];
        assert!(validate_settings(&settings).is_ok());
    }

    #[test]
    fn test_validate_accepts_valid_ssh_connection() {
        let mut settings = AppSettings::default();
        settings.ssh_connections = vec![SshConnection {
            name: "Production".to_string(),
            hostname: "prod.example.com".to_string(),
            port: 22,
            username: "deploy".to_string(),
            identity_file: "~/.ssh/id_ed25519".to_string(),
            ssh_options: Vec::new(),
            extra_options: String::new(),
        }];
        assert!(validate_settings(&settings).is_ok());
    }

    // Profile with wsl_distro_name round-trip
    #[test]
    fn test_profile_wsl_distro_name_round_trip() {
        let profile = Profile {
            name: "WSL Ubuntu".to_string(),
            shell_path: String::new(),
            shell_args: vec![],
            env_vars: String::new(),
            working_directory: String::new(),
            is_default: false,
            ssh_connection_name: String::new(),
            wsl_distro_name: "Ubuntu-22.04".to_string(),
        };
        let json = serde_json::to_string(&profile).unwrap();
        let restored: Profile = serde_json::from_str(&json).unwrap();
        assert_eq!(restored.wsl_distro_name, "Ubuntu-22.04");
        assert_eq!(restored.ssh_connection_name, "");
    }

    // Profile wsl_distro_name defaults to empty
    #[test]
    fn test_profile_wsl_distro_name_default() {
        let json = r#"{"name": "Test"}"#;
        let profile: Profile = serde_json::from_str(json).unwrap();
        assert_eq!(profile.wsl_distro_name, "");
    }
}
