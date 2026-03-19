//! Tauri commands for WSL operations.

/// Detects installed WSL distributions.
///
/// Executes `wsl.exe --list --quiet` and returns a list of distribution names.
/// Returns an empty list on Linux or if WSL is not installed.
/// Failures from wsl.exe are returned as an empty list, not as errors.
#[cfg(feature = "gui")]
#[tauri::command]
pub async fn detect_wsl_distributions() -> Result<Vec<String>, String> {
    #[cfg(windows)]
    {
        tokio::task::spawn_blocking(|| crate::wsl::detect::list_distributions())
            .await
            .map_err(|e| e.to_string())
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
    std::env::consts::OS.to_string()
}
