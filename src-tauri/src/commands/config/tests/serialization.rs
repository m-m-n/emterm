//! Serialization tests (lowercase enum form, shell_args round-trip).

use super::*;

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

#[test]
fn test_shell_args_round_trip() {
    let json = r#"{"shell_args": ["--login", "-i"]}"#;
    let settings: AppSettings = serde_json::from_str(json).unwrap();
    assert_eq!(settings.shell_args, vec!["--login", "-i"]);

    let serialized = serde_json::to_string(&settings).unwrap();
    let restored: AppSettings = serde_json::from_str(&serialized).unwrap();
    assert_eq!(restored.shell_args, vec!["--login", "-i"]);
}
