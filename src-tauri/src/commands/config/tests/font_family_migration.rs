//! Tests for legacy `font_family` migration to the three-field
//! (primary / secondary / emoji) layout.

use super::*;

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
