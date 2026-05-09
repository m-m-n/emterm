//! Generic deserialization tests covering empty JSON, null values,
//! unknown fields, and invalid enum values.

use super::*;

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
