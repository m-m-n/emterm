//! Tests for `show_tab_bar` and the `toggle_tab_bar` keybind.

use super::*;

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
