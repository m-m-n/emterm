//! Default value tests for `AppSettings` and `KeybindSettings`.

use super::*;

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
    assert_eq!(keybinds.new_tab_global, "Ctrl+Shift+G");
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
