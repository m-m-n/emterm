use std::fs;
use std::path::PathBuf;

use tauri::AppHandle;
use tauri::Manager;

use super::settings::AppSettings;
use super::validation::validate_settings;

// ============================================================
// Commands
// ============================================================

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
///
/// All fields always have valid values due to serde(default) + deserialize_null_default.
#[tauri::command]
pub fn load_settings(app: AppHandle) -> Result<AppSettings, String> {
    let config_path = get_config_path(&app)?;

    // If file doesn't exist, return defaults
    if !config_path.exists() {
        return Ok(AppSettings::default());
    }

    // Read file contents
    let contents = match fs::read_to_string(&config_path) {
        Ok(c) => c,
        Err(e) => {
            log::warn!("Failed to read settings file: {}", e);
            return Ok(AppSettings::default());
        }
    };

    // Parse JSON — serde(default) handles missing fields,
    // deserialize_null_default handles null values
    match serde_json::from_str::<AppSettings>(&contents) {
        Ok(mut settings) => {
            // Migration: move legacy font_family to font_family_primary if needed
            if !settings.font_family.is_empty() && settings.font_family_primary.is_empty() {
                settings.font_family_primary = std::mem::take(&mut settings.font_family);
            } else {
                settings.font_family.clear();
            }

            Ok(settings)
        }
        Err(e) => {
            log::warn!("Failed to parse settings file: {}", e);
            Ok(AppSettings::default())
        }
    }
}

/// Saves application settings to the config file.
///
/// Returns an error if:
/// - Any field fails validation
/// - The config directory cannot be created
/// - The file cannot be written
#[tauri::command]
pub fn save_settings(app: AppHandle, settings: AppSettings) -> Result<(), String> {
    validate_settings(&settings)?;

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
