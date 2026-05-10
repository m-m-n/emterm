//! Tests for notification settings.

use super::*;

#[test]
fn test_deserialize_missing_notification_fields_use_defaults() {
    let json = r#"{}"#;
    let settings: AppSettings = serde_json::from_str(json).unwrap();
    assert!(settings.notification_enabled);
    assert!(settings.tab_activity_indicator);
    assert!(settings.notify_on_process_exit);
    assert!(!settings.notify_on_output);
    assert!(settings.notify_on_bell);
}

#[test]
fn test_deserialize_null_notification_fields_use_defaults() {
    let json = r#"{
        "notification_enabled": null,
        "tab_activity_indicator": null,
        "notify_on_process_exit": null,
        "notify_on_output": null,
        "notify_on_bell": null
    }"#;
    let settings: AppSettings = serde_json::from_str(json).unwrap();
    assert!(settings.notification_enabled);
    assert!(settings.tab_activity_indicator);
    assert!(settings.notify_on_process_exit);
    assert!(!settings.notify_on_output);
    assert!(settings.notify_on_bell);
}

#[test]
fn test_notification_settings_round_trip() {
    let mut settings = AppSettings::default();
    settings.notification_enabled = false;
    settings.tab_activity_indicator = false;
    settings.notify_on_process_exit = false;
    settings.notify_on_output = true;
    settings.notify_on_bell = false;
    let json = serde_json::to_string(&settings).unwrap();
    let restored: AppSettings = serde_json::from_str(&json).unwrap();
    assert!(!restored.notification_enabled);
    assert!(!restored.tab_activity_indicator);
    assert!(!restored.notify_on_process_exit);
    assert!(restored.notify_on_output);
    assert!(!restored.notify_on_bell);
}
