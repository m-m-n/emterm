//! Tests for status bar settings.

use super::*;

#[test]
fn test_statusbar_defaults() {
    let settings = AppSettings::default();
    assert!(!settings.statusbar_enabled);
    assert_eq!(settings.statusbar_app_line1_left, "{time}");
    assert_eq!(settings.statusbar_app_line1_right, "{cwd}");
    assert_eq!(settings.statusbar_app_line2_left, "");
    assert_eq!(settings.statusbar_app_line2_right, "");
    assert_eq!(settings.statusbar_time_format, "HH:mm:ss");
    assert!(settings.statusbar_custom_commands.is_empty());
    assert!(settings.statusbar_refresh_rates.is_empty());
}

#[test]
fn test_statusbar_deserialize_empty_json() {
    let json = r#"{}"#;
    let settings: AppSettings = serde_json::from_str(json).unwrap();
    assert!(!settings.statusbar_enabled);
    assert_eq!(settings.statusbar_app_line1_left, "{time}");
    assert_eq!(settings.statusbar_app_line1_right, "{cwd}");
}

#[test]
fn test_statusbar_deserialize_null_values() {
    let json = r#"{
        "statusbar_enabled": null,
        "statusbar_app_line1_left": null,
        "statusbar_app_line1_right": null,
        "statusbar_time_format": null
    }"#;
    let settings: AppSettings = serde_json::from_str(json).unwrap();
    assert!(!settings.statusbar_enabled);
    assert_eq!(settings.statusbar_app_line1_left, "{time}");
    assert_eq!(settings.statusbar_app_line1_right, "{cwd}");
    assert_eq!(settings.statusbar_time_format, "HH:mm:ss");
}

#[test]
fn test_statusbar_deserialize_custom_values() {
    let json = r#"{
        "statusbar_enabled": true,
        "statusbar_app_line1_left": "{git_branch}",
        "statusbar_app_line1_right": "{time} | {cwd}"
    }"#;
    let settings: AppSettings = serde_json::from_str(json).unwrap();
    assert!(settings.statusbar_enabled);
    assert_eq!(settings.statusbar_app_line1_left, "{git_branch}");
    assert_eq!(settings.statusbar_app_line1_right, "{time} | {cwd}");
}

#[test]
fn test_statusbar_custom_commands_deserialize() {
    let json = r#"{
        "statusbar_custom_commands": {
            "uptime": { "executable": "/usr/bin/uptime", "interval_ms": 5000 },
            "load": { "executable": "/usr/local/bin/load" }
        }
    }"#;
    let settings: AppSettings = serde_json::from_str(json).unwrap();
    assert_eq!(settings.statusbar_custom_commands.len(), 2);
    let uptime = settings.statusbar_custom_commands.get("uptime").unwrap();
    assert_eq!(uptime.executable, "/usr/bin/uptime");
    assert_eq!(uptime.interval_ms, 5000);
    let load = settings.statusbar_custom_commands.get("load").unwrap();
    assert_eq!(load.executable, "/usr/local/bin/load");
    assert_eq!(load.interval_ms, 1000); // default
}

#[test]
fn test_statusbar_validation_command_with_args() {
    let mut settings = AppSettings::default();
    settings.statusbar_custom_commands.insert(
        "test".to_string(),
        StatusbarCustomCommand {
            executable: "/usr/bin/test -f".to_string(),
            interval_ms: 1000,
        },
    );
    assert!(validate_settings(&settings).is_err());
}

#[test]
fn test_statusbar_validation_command_empty_executable() {
    let mut settings = AppSettings::default();
    settings.statusbar_custom_commands.insert(
        "test".to_string(),
        StatusbarCustomCommand {
            executable: "".to_string(),
            interval_ms: 1000,
        },
    );
    assert!(validate_settings(&settings).is_err());
}

#[test]
fn test_statusbar_validation_font_size_out_of_range() {
    let mut settings = AppSettings::default();
    settings.statusbar_font_size = Some(100.0);
    assert!(validate_settings(&settings).is_err());
}

#[test]
fn test_statusbar_validation_valid() {
    let mut settings = AppSettings::default();
    settings.statusbar_enabled = true;
    settings.statusbar_custom_commands.insert(
        "uptime".to_string(),
        StatusbarCustomCommand {
            executable: "/usr/bin/uptime".to_string(),
            interval_ms: 5000,
        },
    );
    assert!(validate_settings(&settings).is_ok());
}
