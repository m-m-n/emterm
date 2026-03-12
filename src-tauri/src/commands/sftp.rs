//! Tauri commands for SFTP upload operations.

use crate::sftp::check::find_duplicates;
use crate::sftp::pool::ConcurrentUploadPool;
use crate::sftp::upload::SftpProcessManager;
use crate::sftp::{SftpUploadProgress, SftpUploadStatus};

/// SSH connection details passed from the frontend for sftp operations.
#[derive(Debug, serde::Deserialize)]
pub struct SftpConnectionArgs {
    pub hostname: String,
    pub port: u16,
    pub username: String,
    pub identity_file: String,
    pub ssh_options: Vec<SftpOptionArg>,
}

/// SSH option key-value pair.
#[derive(Debug, serde::Deserialize)]
pub struct SftpOptionArg {
    pub key: String,
    pub value: String,
}

impl SftpConnectionArgs {
    fn to_option_tuples(&self) -> Vec<(String, String)> {
        self.ssh_options
            .iter()
            .map(|o| (o.key.clone(), o.value.clone()))
            .collect()
    }
}

/// Check for duplicate files on the remote host.
///
/// Spawns an sftp subprocess, runs `ls` on the remote directory,
/// and returns a list of file names that already exist.
#[cfg(feature = "gui")]
#[tauri::command]
pub fn sftp_check_duplicates(
    connection: SftpConnectionArgs,
    remote_dir: String,
    file_names: Vec<String>,
    sftp_manager: tauri::State<'_, SftpProcessManager>,
) -> Result<Vec<String>, String> {
    // Validate inputs
    validate_connection(&connection)?;
    validate_remote_path(&remote_dir)?;

    let sftp_binary = get_sftp_binary()?;
    let ssh_options = connection.to_option_tuples();

    let ls_output = sftp_manager.spawn_ls(
        &sftp_binary,
        &connection.hostname,
        connection.port,
        &connection.username,
        &connection.identity_file,
        &ssh_options,
        &remote_dir,
    )?;

    Ok(find_duplicates(&ls_output, &file_names))
}

/// Upload a file or directory to the remote host via sftp.
///
/// Validates inputs synchronously, then spawns a background thread for the
/// actual transfer. Returns immediately so the UI remains responsive.
/// Progress and completion are reported via `sftp-upload-progress` events.
#[cfg(feature = "gui")]
#[tauri::command]
pub fn sftp_upload(
    session_id: String,
    connection: SftpConnectionArgs,
    local_path: String,
    remote_path: String,
    is_directory: bool,
    app: tauri::AppHandle,
) -> Result<(), String> {
    // Validate inputs synchronously (errors are returned to the caller)
    validate_connection(&connection)?;
    validate_remote_path(&remote_path)?;
    validate_local_path(&local_path)?;

    let sftp_binary = get_sftp_binary()?;
    let ssh_options = connection.to_option_tuples();
    let hostname = connection.hostname;
    let port = connection.port;
    let username = connection.username;
    let identity_file = connection.identity_file;

    // Extract file name for progress reporting
    let file_name = std::path::Path::new(&local_path)
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| local_path.clone());

    // Get file size for progress reporting
    let total_bytes = get_local_size(&local_path, is_directory);

    // Emit preparing event (shown while waiting for pool slot / SSH handshake)
    emit_progress(
        &app,
        SftpUploadProgress {
            session_id: session_id.clone(),
            file_name: file_name.clone(),
            bytes_transferred: 0,
            total_bytes,
            status: SftpUploadStatus::Preparing,
            error_message: None,
        },
    );

    // Spawn background thread for the actual upload
    std::thread::spawn(move || {
        use tauri::Manager;
        let sftp_manager = app.state::<SftpProcessManager>();
        let upload_pool = app.state::<ConcurrentUploadPool>();

        // Acquire a pool slot (blocks if all slots are occupied)
        upload_pool.acquire_slot(&session_id);

        // Emit uploading event (transfer is now in progress)
        emit_progress(
            &app,
            SftpUploadProgress {
                session_id: session_id.clone(),
                file_name: file_name.clone(),
                bytes_transferred: 0,
                total_bytes,
                status: SftpUploadStatus::Uploading,
                error_message: None,
            },
        );

        let result = sftp_manager.spawn_upload(
            &session_id,
            &sftp_binary,
            &hostname,
            port,
            &username,
            &identity_file,
            &ssh_options,
            &local_path,
            &remote_path,
            is_directory,
        );

        // Release the pool slot regardless of outcome
        upload_pool.release_slot(&session_id);

        match result {
            Ok(_output) => {
                emit_progress(
                    &app,
                    SftpUploadProgress {
                        session_id,
                        file_name,
                        bytes_transferred: total_bytes,
                        total_bytes,
                        status: SftpUploadStatus::Completed,
                        error_message: None,
                    },
                );
            }
            Err(e) => {
                let status = if e.contains("cancelled") {
                    SftpUploadStatus::Cancelled
                } else {
                    SftpUploadStatus::Failed
                };
                emit_progress(
                    &app,
                    SftpUploadProgress {
                        session_id,
                        file_name,
                        bytes_transferred: 0,
                        total_bytes,
                        status,
                        error_message: Some(e),
                    },
                );
            }
        }
    });

    Ok(())
}

/// Cancel an in-progress upload by killing its sftp subprocess.
#[cfg(feature = "gui")]
#[tauri::command]
pub fn sftp_cancel_upload(
    session_id: String,
    sftp_manager: tauri::State<'_, SftpProcessManager>,
    upload_pool: tauri::State<'_, ConcurrentUploadPool>,
) -> Result<(), String> {
    let result = sftp_manager.cancel(&session_id);
    // Release the slot even if cancel fails (process may have already exited)
    upload_pool.release_slot(&session_id);
    result
}

// ============================================================
// Helper Functions
// ============================================================

/// Get the total size of a local file or directory in bytes.
fn get_local_size(path: &str, is_directory: bool) -> u64 {
    if is_directory {
        dir_size(std::path::Path::new(path))
    } else {
        std::fs::metadata(path).map(|m| m.len()).unwrap_or(0)
    }
}

/// Recursively compute the total size of a directory.
fn dir_size(path: &std::path::Path) -> u64 {
    let mut total: u64 = 0;
    if let Ok(entries) = std::fs::read_dir(path) {
        for entry in entries.flatten() {
            let meta = match entry.metadata() {
                Ok(m) => m,
                Err(_) => continue,
            };
            if meta.is_dir() {
                total += dir_size(&entry.path());
            } else {
                total += meta.len();
            }
        }
    }
    total
}

/// Validate SSH connection arguments.
fn validate_connection(connection: &SftpConnectionArgs) -> Result<(), String> {
    if connection.hostname.is_empty() {
        return Err("Missing hostname".to_string());
    }
    // Reject hostnames with shell metacharacters
    if connection
        .hostname
        .chars()
        .any(|c| matches!(c, ';' | '|' | '&' | '`' | '$' | '(' | ')' | '{' | '}'))
    {
        return Err("Invalid hostname: contains shell metacharacters".to_string());
    }
    Ok(())
}

/// Validate remote path to prevent directory traversal and injection.
fn validate_remote_path(path: &str) -> Result<(), String> {
    // Reject paths with null bytes
    if path.contains('\0') {
        return Err("Invalid remote path: contains null bytes".to_string());
    }
    // Reject characters dangerous in sftp batch mode:
    // - Shell metacharacters that could be used for command injection
    // - Double quotes and newlines that could escape sftp command quoting
    if path.chars().any(|c| {
        matches!(
            c,
            ';' | '|' | '&' | '`' | '$' | '(' | ')' | '"' | '\\' | '\n' | '\r'
        )
    }) {
        return Err("Invalid remote path: contains unsafe characters".to_string());
    }
    Ok(())
}

/// Validate local file path exists.
fn validate_local_path(path: &str) -> Result<(), String> {
    if path.is_empty() {
        return Err("Local path is empty".to_string());
    }
    if path.contains('\0') {
        return Err("Invalid local path: contains null bytes".to_string());
    }
    // Reject characters that could escape sftp batch command quoting
    // On Windows, backslash is a valid path separator and must be allowed
    #[cfg(windows)]
    let has_unsafe = path.chars().any(|c| matches!(c, '"' | '\n' | '\r'));
    #[cfg(not(windows))]
    let has_unsafe = path.chars().any(|c| matches!(c, '"' | '\\' | '\n' | '\r'));
    if has_unsafe {
        return Err("Invalid local path: contains unsafe characters".to_string());
    }
    if !std::path::Path::new(path).exists() {
        return Err(format!("Local path does not exist: {}", path));
    }
    Ok(())
}

/// Get the sftp binary path, checking availability.
fn get_sftp_binary() -> Result<String, String> {
    let path = detect_sftp_binary();
    if path.is_empty() {
        Err("sftp command not found. Ensure openssh is installed.".to_string())
    } else {
        Ok(path)
    }
}

/// Detect the sftp binary path on the current platform.
fn detect_sftp_binary() -> String {
    #[cfg(unix)]
    {
        detect_sftp_unix()
    }
    #[cfg(windows)]
    {
        detect_sftp_windows()
    }
}

#[cfg(unix)]
fn detect_sftp_unix() -> String {
    let path_var = match std::env::var("PATH") {
        Ok(p) => p,
        Err(_) => return String::new(),
    };
    for dir in path_var.split(':') {
        let candidate = std::path::PathBuf::from(dir).join("sftp");
        if candidate.is_file() {
            return candidate.to_string_lossy().to_string();
        }
    }
    String::new()
}

#[cfg(windows)]
fn detect_sftp_windows() -> String {
    let system32_path = std::path::PathBuf::from(r"C:\Windows\System32\OpenSSH\sftp.exe");
    if system32_path.is_file() {
        return system32_path.to_string_lossy().to_string();
    }
    let path_var = match std::env::var("PATH") {
        Ok(p) => p,
        Err(_) => return String::new(),
    };
    for dir in path_var.split(';') {
        let candidate = std::path::PathBuf::from(dir).join("sftp.exe");
        if candidate.is_file() {
            return candidate.to_string_lossy().to_string();
        }
    }
    String::new()
}

/// Emit an SFTP upload progress event to the frontend.
#[cfg(feature = "gui")]
fn emit_progress(app: &tauri::AppHandle, progress: SftpUploadProgress) {
    use tauri::Emitter;
    if let Err(e) = app.emit("sftp-upload-progress", &progress) {
        log::error!("Failed to emit sftp-upload-progress: {}", e);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detect_sftp_binary_returns_string() {
        let result = detect_sftp_binary();
        let _ = result;
    }

    #[cfg(unix)]
    #[test]
    fn test_detect_sftp_on_unix() {
        let result = detect_sftp_binary();
        if !result.is_empty() {
            assert!(
                result.contains("sftp"),
                "Result should contain 'sftp': {}",
                result
            );
        }
    }

    #[test]
    fn test_get_sftp_binary_success() {
        let result = get_sftp_binary();
        let _ = result;
    }

    #[test]
    fn test_connection_args_to_option_tuples() {
        let args = SftpConnectionArgs {
            hostname: "example.com".to_string(),
            port: 22,
            username: "user".to_string(),
            identity_file: String::new(),
            ssh_options: vec![SftpOptionArg {
                key: "StrictHostKeyChecking".to_string(),
                value: "no".to_string(),
            }],
        };
        let tuples = args.to_option_tuples();
        assert_eq!(tuples.len(), 1);
        assert_eq!(tuples[0].0, "StrictHostKeyChecking");
        assert_eq!(tuples[0].1, "no");
    }

    #[test]
    fn test_get_local_size_file() {
        // Use Cargo.toml as a known-existing file
        let size = get_local_size("Cargo.toml", false);
        assert!(size > 0);
    }

    #[test]
    fn test_get_local_size_nonexistent() {
        let size = get_local_size("/nonexistent/path", false);
        assert_eq!(size, 0);
    }

    #[test]
    fn test_get_local_size_directory() {
        let size = get_local_size("src", true);
        assert!(size > 0);
    }

    #[test]
    fn test_dir_size_nonexistent() {
        let size = dir_size(std::path::Path::new("/nonexistent"));
        assert_eq!(size, 0);
    }
}
