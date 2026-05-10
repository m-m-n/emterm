//! Tests for the `ui_font_family` field.

use super::*;

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
