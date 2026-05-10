//! Tests for Markdown viewer settings (font / size / theme).

use super::*;

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
