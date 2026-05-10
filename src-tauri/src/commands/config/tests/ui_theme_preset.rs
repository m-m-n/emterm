//! Tests for the `UiThemePreset` enum.

use super::*;

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
