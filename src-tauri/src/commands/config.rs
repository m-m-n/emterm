use std::env;
use std::fs;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use tauri::AppHandle;
use tauri::Manager;

/// Default font size in points
pub const DEFAULT_FONT_SIZE: u32 = 13;

/// Minimum allowed font size
pub const MIN_FONT_SIZE: u32 = 8;

/// Maximum allowed font size
pub const MAX_FONT_SIZE: u32 = 32;

/// Application settings structure for JSON serialization
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppSettings {
    /// Font size in points (8-32)
    #[serde(default = "default_font_size")]
    pub font_size: u32,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            font_size: DEFAULT_FONT_SIZE,
        }
    }
}

/// Returns the default font size for serde default
fn default_font_size() -> u32 {
    DEFAULT_FONT_SIZE
}

/// Internal structure for parsing JSON that may have null values
#[derive(Debug, Deserialize)]
struct AppSettingsFile {
    #[serde(default)]
    font_size: Option<u32>,
}

/// Get the renderer type from environment variable at runtime.
///
/// This allows the frontend to check `EMTERM_RENDERER` environment variable
/// at runtime, enabling E2E tests to verify renderer switching.
#[tauri::command]
pub fn get_renderer_type() -> String {
    env::var("EMTERM_RENDERER").unwrap_or_else(|_| "dom".to_string())
}

/// Get the config directory path for settings
fn get_config_path(app: &AppHandle) -> Result<PathBuf, String> {
    let config_dir = app
        .path()
        .app_config_dir()
        .map_err(|e| format!("Failed to get config directory: {}", e))?;

    Ok(config_dir.join("settings.json"))
}

/// Loads application settings from the config file.
///
/// Returns default settings if:
/// - The file doesn't exist
/// - The file cannot be parsed
/// - The font_size field is null or missing
///
/// The font_size value is always valid (never null) in the response.
#[tauri::command]
pub fn load_settings(app: AppHandle) -> Result<AppSettings, String> {
    let config_path = get_config_path(&app)?;

    // If file doesn't exist, return defaults
    if !config_path.exists() {
        return Ok(AppSettings {
            font_size: DEFAULT_FONT_SIZE,
        });
    }

    // Read file contents
    let contents = match fs::read_to_string(&config_path) {
        Ok(c) => c,
        Err(e) => {
            log::warn!("Failed to read settings file: {}", e);
            return Ok(AppSettings {
                font_size: DEFAULT_FONT_SIZE,
            });
        }
    };

    // Parse JSON with optional fields
    let file_settings: AppSettingsFile = match serde_json::from_str(&contents) {
        Ok(s) => s,
        Err(e) => {
            log::warn!("Failed to parse settings file: {}", e);
            return Ok(AppSettings {
                font_size: DEFAULT_FONT_SIZE,
            });
        }
    };

    // Apply defaults for null/missing values
    Ok(AppSettings {
        font_size: file_settings.font_size.unwrap_or(DEFAULT_FONT_SIZE),
    })
}

/// Saves application settings to the config file.
///
/// Returns an error if:
/// - The font_size is outside the valid range (8-32)
/// - The config directory cannot be created
/// - The file cannot be written
#[tauri::command]
pub fn save_settings(app: AppHandle, settings: AppSettings) -> Result<(), String> {
    // Validate font_size range
    if settings.font_size < MIN_FONT_SIZE || settings.font_size > MAX_FONT_SIZE {
        return Err(format!(
            "font_size must be between {} and {}",
            MIN_FONT_SIZE, MAX_FONT_SIZE
        ));
    }

    let config_path = get_config_path(&app)?;

    // Create config directory if it doesn't exist
    if let Some(parent) = config_path.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| format!("Failed to create config directory: {}", e))?;
    }

    // Serialize settings to JSON
    let json = serde_json::to_string_pretty(&settings)
        .map_err(|e| format!("Failed to serialize settings: {}", e))?;

    // Write to file
    fs::write(&config_path, json).map_err(|e| format!("Failed to write settings file: {}", e))?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_font_size_value() {
        assert_eq!(DEFAULT_FONT_SIZE, 13);
    }

    #[test]
    fn test_app_settings_default() {
        let settings = AppSettings::default();
        assert_eq!(settings.font_size, DEFAULT_FONT_SIZE);
    }

    #[test]
    fn test_app_settings_serialization() {
        let settings = AppSettings { font_size: 20 };
        let json = serde_json::to_string(&settings).unwrap();
        assert!(json.contains("\"font_size\":20"));
    }

    #[test]
    fn test_app_settings_deserialization_with_value() {
        let json = r#"{"font_size": 24}"#;
        let settings: AppSettings = serde_json::from_str(json).unwrap();
        assert_eq!(settings.font_size, 24);
    }

    #[test]
    fn test_app_settings_deserialization_missing_field() {
        let json = r#"{}"#;
        let settings: AppSettings = serde_json::from_str(json).unwrap();
        assert_eq!(settings.font_size, DEFAULT_FONT_SIZE);
    }

    #[test]
    fn test_app_settings_file_with_null() {
        let json = r#"{"font_size": null}"#;
        let file_settings: AppSettingsFile = serde_json::from_str(json).unwrap();
        assert_eq!(file_settings.font_size, None);
    }

    #[test]
    fn test_app_settings_file_with_value() {
        let json = r#"{"font_size": 18}"#;
        let file_settings: AppSettingsFile = serde_json::from_str(json).unwrap();
        assert_eq!(file_settings.font_size, Some(18));
    }

    #[test]
    fn test_app_settings_file_missing_field() {
        let json = r#"{}"#;
        let file_settings: AppSettingsFile = serde_json::from_str(json).unwrap();
        assert_eq!(file_settings.font_size, None);
    }

    #[test]
    fn test_settings_round_trip() {
        let settings = AppSettings { font_size: 14 };
        let json = serde_json::to_string(&settings).unwrap();
        let restored: AppSettings = serde_json::from_str(&json).unwrap();
        assert_eq!(restored.font_size, 14);
    }

    #[test]
    fn test_font_size_validation_below_minimum() {
        let settings = AppSettings { font_size: 7 };
        // Validation is done in save_settings, here we just test the struct can hold any value
        assert_eq!(settings.font_size, 7);
    }

    #[test]
    fn test_font_size_validation_above_maximum() {
        let settings = AppSettings { font_size: 33 };
        // Validation is done in save_settings, here we just test the struct can hold any value
        assert_eq!(settings.font_size, 33);
    }

    #[test]
    fn test_font_size_valid_range_minimum() {
        let settings = AppSettings {
            font_size: MIN_FONT_SIZE,
        };
        assert_eq!(settings.font_size, 8);
    }

    #[test]
    fn test_font_size_valid_range_maximum() {
        let settings = AppSettings {
            font_size: MAX_FONT_SIZE,
        };
        assert_eq!(settings.font_size, 32);
    }

    // The following tests require a Tauri AppHandle which is not easily mockable
    // in unit tests. These behaviors should be verified via integration tests
    // or manual testing.

    // Integration test scenarios:
    // - load_settings returns DEFAULT_FONT_SIZE when file missing
    // - load_settings returns DEFAULT_FONT_SIZE when font_size is null in file
    // - load_settings parses valid JSON and returns saved font_size
    // - save_settings creates directory if missing
    // - save_settings writes valid JSON
    // - save_settings rejects font_size below 8
    // - save_settings rejects font_size above 32
    // - save_settings accepts font_size in valid range (8-32)
    // - Round-trip: save then load
}
