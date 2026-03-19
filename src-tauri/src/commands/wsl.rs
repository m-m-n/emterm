//! Tauri commands for WSL operations.

/// Detects installed WSL distributions.
///
/// Executes `wsl.exe --list --quiet` and returns a list of distribution names.
/// Returns an empty list on Linux or if WSL is not installed.
#[cfg(feature = "gui")]
#[tauri::command]
pub fn detect_wsl_distributions() -> Result<Vec<String>, String> {
    #[cfg(windows)]
    {
        Ok(crate::wsl::detect::list_distributions())
    }
    #[cfg(not(windows))]
    {
        Ok(Vec::new())
    }
}

/// Returns the current platform identifier.
///
/// Returns "windows" on Windows, "linux" on Linux.
#[cfg(feature = "gui")]
#[tauri::command]
pub fn get_platform() -> String {
    if cfg!(windows) {
        "windows".to_string()
    } else {
        "linux".to_string()
    }
}
