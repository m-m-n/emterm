//! Tests for the `language` field deserialization.

use super::*;

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
