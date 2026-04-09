//! Auto-import tmux.conf settings on first mux startup.

/// Import tmux.conf settings on first mux startup.
///
/// Reads the eMterm settings file directly (without AppHandle),
/// checks `tmux_conf_imported` flag, and applies tmux.conf conversions.
pub(super) fn import_tmux_conf_if_needed() {
    let settings_path = match settings_file_path() {
        Some(p) => p,
        None => return,
    };

    let mut settings = if settings_path.exists() {
        match std::fs::read_to_string(&settings_path) {
            Ok(contents) => serde_json::from_str::<crate::commands::config::AppSettings>(&contents)
                .unwrap_or_default(),
            Err(_) => crate::commands::config::AppSettings::default(),
        }
    } else {
        crate::commands::config::AppSettings::default()
    };

    // Skip if already imported
    if settings.mux.tmux_conf_imported {
        return;
    }

    // Mark as imported (even if no tmux.conf exists, don't retry)
    settings.mux.tmux_conf_imported = true;

    // Try auto-import
    if let Some(result) = super::tmux_conf::converter::auto_import_tmux_conf() {
        for (key, value) in &result.settings {
            match key.as_str() {
                "prefix" => settings.mux.prefix = value.clone(),
                "base_index" => {
                    if let Ok(v) = value.parse::<u32>() {
                        settings.mux.base_index = v;
                    }
                }
                "mouse" => {
                    settings.mux.mouse = value == "true";
                }
                "status_position" => {
                    settings.mux.status_position = value.clone();
                }
                k if k.starts_with("keybind.") => {
                    let bind_key = k.strip_prefix("keybind.").unwrap().to_string();
                    settings.mux.keybinds.insert(bind_key, value.clone());
                }
                _ => {}
            }
        }

        for warning in &result.warnings {
            log::warn!("tmux.conf import: {}", warning);
        }

        if !result.settings.is_empty() {
            log::info!(
                "tmux.conf: imported {} settings ({} warnings)",
                result.settings.len(),
                result.warnings.len()
            );
        }
    }

    // Save settings
    if let Some(parent) = settings_path.parent() {
        if let Err(e) = std::fs::create_dir_all(parent) {
            log::warn!("Failed to create config directory: {}", e);
            return;
        }
    }
    match serde_json::to_string_pretty(&settings) {
        Ok(json) => {
            if let Err(e) = std::fs::write(&settings_path, json) {
                log::warn!("Failed to save settings after tmux.conf import: {}", e);
            }
        }
        Err(e) => {
            log::warn!("Failed to serialize settings: {}", e);
        }
    }
}

/// Resolve the eMterm settings file path without AppHandle.
///
/// Uses XDG_CONFIG_HOME or ~/.config as base, matching Tauri's
/// `app_config_dir()` behavior on Linux.
fn settings_file_path() -> Option<std::path::PathBuf> {
    let config_base = std::env::var("XDG_CONFIG_HOME")
        .ok()
        .filter(|s| !s.is_empty())
        .map(std::path::PathBuf::from)
        .or_else(|| {
            std::env::var("HOME")
                .ok()
                .map(|h| std::path::PathBuf::from(h).join(".config"))
        })?;
    Some(
        config_base
            .join("net.laser5.app.emterm")
            .join("settings.json"),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_settings_file_path_with_home() {
        temp_env::with_vars(
            [
                ("XDG_CONFIG_HOME", None::<&str>),
                ("HOME", Some("/tmp/test_home")),
            ],
            || {
                let path = settings_file_path().unwrap();
                assert_eq!(
                    path,
                    std::path::PathBuf::from(
                        "/tmp/test_home/.config/net.laser5.app.emterm/settings.json"
                    )
                );
            },
        );
    }

    #[test]
    fn test_settings_file_path_with_xdg() {
        temp_env::with_var("XDG_CONFIG_HOME", Some("/tmp/xdg_config"), || {
            let path = settings_file_path().unwrap();
            assert_eq!(
                path,
                std::path::PathBuf::from("/tmp/xdg_config/net.laser5.app.emterm/settings.json")
            );
        });
    }
}
