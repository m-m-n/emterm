//! Tests for `UserColorScheme` and the `custom_color_schemes` field.

use super::*;

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
