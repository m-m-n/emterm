//! Tauri commands for SSH operations.

use crate::ssh;

/// Detects the SSH binary path on the system.
///
/// Returns the full path to the ssh binary, or empty string if not found.
#[cfg(feature = "gui")]
#[tauri::command]
pub fn detect_ssh_command() -> Result<String, String> {
    Ok(ssh::detect::detect_ssh_command())
}

/// Loads host entries from ~/.ssh/config with per-host directives.
///
/// Returns a list of SshConfigHost entries with host alias, hostname, port, user, identity_file.
/// Returns empty list if the file does not exist.
#[cfg(feature = "gui")]
#[tauri::command]
pub fn load_ssh_config_hosts() -> Result<Vec<ssh::config::SshConfigHost>, String> {
    let path = ssh::config::default_ssh_config_path();
    match path {
        Some(p) => Ok(ssh::config::parse_ssh_config(&p)),
        None => Ok(Vec::new()),
    }
}

/// Builds SSH command arguments from connection settings.
///
/// Returns an array of arguments to pass to the ssh binary.
#[cfg(feature = "gui")]
#[tauri::command]
pub fn build_ssh_args(
    hostname: String,
    port: u16,
    username: String,
    identity_file: String,
    ssh_options: Vec<ssh::detect::SshOptionArg>,
) -> Result<Vec<String>, String> {
    let opts: Vec<(String, String)> = ssh_options.into_iter().map(|o| (o.key, o.value)).collect();
    Ok(ssh::detect::build_ssh_args(
        &hostname,
        port,
        &username,
        &identity_file,
        &opts,
    ))
}

/// Validates that an identity file exists.
///
/// Expands `~` to the home directory before checking.
/// Returns true if the file exists, false otherwise.
#[cfg(feature = "gui")]
#[tauri::command]
pub fn validate_identity_file(path: String) -> Result<bool, String> {
    let expanded = ssh::detect::expand_tilde(&path);
    Ok(std::path::Path::new(&expanded).is_file())
}
