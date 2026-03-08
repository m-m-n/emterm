//! SFTP subprocess lifecycle management.
//!
//! Manages spawning, stdin command writing, and killing of sftp subprocesses.
//! Reads stderr for error detection via the progress module's `parse_error_line`.

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::process::{Child, Command, Stdio};
use std::sync::Mutex;

use crate::sftp::args::build_sftp_args;
use crate::sftp::progress::parse_error_line;

/// Manages active sftp subprocess sessions.
pub struct SftpProcessManager {
    processes: Mutex<HashMap<String, Child>>,
}

impl SftpProcessManager {
    pub fn new() -> Self {
        Self {
            processes: Mutex::new(HashMap::new()),
        }
    }

    /// Spawn an sftp subprocess and send an upload command.
    ///
    /// Streams stderr for error detection. Returns file size (from local metadata)
    /// and any collected error messages.
    pub fn spawn_upload(
        &self,
        session_id: &str,
        sftp_binary: &str,
        hostname: &str,
        port: u16,
        username: &str,
        identity_file: &str,
        ssh_options: &[(String, String)],
        local_path: &str,
        remote_path: &str,
        is_directory: bool,
    ) -> Result<String, String> {
        let args = build_sftp_args(hostname, port, username, identity_file, ssh_options);

        let mut child = Command::new(sftp_binary)
            .args(&args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| format!("Failed to spawn sftp: {}", e))?;

        // Write put command to stdin
        let put_cmd = if is_directory {
            format!("put -r \"{}\" \"{}\"\nbye\n", local_path, remote_path)
        } else {
            format!("put \"{}\" \"{}\"\nbye\n", local_path, remote_path)
        };

        if let Some(stdin) = child.stdin.as_mut() {
            stdin
                .write_all(put_cmd.as_bytes())
                .map_err(|e| format!("Failed to write to sftp stdin: {}", e))?;
        }
        // Drop stdin to signal EOF
        drop(child.stdin.take());

        // Take stderr handle before storing child for cancellation
        let stderr_handle = child.stderr.take();

        // Store the child process for potential cancellation
        {
            let mut processes = self.processes.lock().unwrap();
            processes.insert(session_id.to_string(), child);
        }

        // Read stderr for error detection
        let mut stderr_errors: Vec<String> = Vec::new();
        if let Some(stderr) = stderr_handle {
            let reader = BufReader::new(stderr);
            for line in reader.lines() {
                if let Ok(line) = line {
                    if let Some(error_msg) = parse_error_line(&line) {
                        stderr_errors.push(error_msg);
                    }
                }
            }
        }

        // Wait for the process to complete
        let mut processes = self.processes.lock().unwrap();
        if let Some(mut child) = processes.remove(session_id) {
            let status = child
                .wait()
                .map_err(|e| format!("Failed to wait for sftp: {}", e))?;

            if status.success() {
                Ok(String::new())
            } else if !stderr_errors.is_empty() {
                Err(stderr_errors.join("\n"))
            } else {
                Err(format!("sftp exited with status: {}", status))
            }
        } else {
            // Process was cancelled while we were waiting
            Err("Upload cancelled".to_string())
        }
    }

    /// Spawn an sftp subprocess to list files in a remote directory.
    pub fn spawn_ls(
        &self,
        sftp_binary: &str,
        hostname: &str,
        port: u16,
        username: &str,
        identity_file: &str,
        ssh_options: &[(String, String)],
        remote_dir: &str,
    ) -> Result<String, String> {
        let args = build_sftp_args(hostname, port, username, identity_file, ssh_options);

        let mut child = Command::new(sftp_binary)
            .args(&args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| format!("Failed to spawn sftp: {}", e))?;

        // Write ls command to stdin
        let ls_cmd = format!("ls \"{}\"\nbye\n", remote_dir);

        if let Some(stdin) = child.stdin.as_mut() {
            stdin
                .write_all(ls_cmd.as_bytes())
                .map_err(|e| format!("Failed to write to sftp stdin: {}", e))?;
        }

        let output = child
            .wait_with_output()
            .map_err(|e| format!("Failed to wait for sftp: {}", e))?;

        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();

        if output.status.success() {
            Ok(stdout)
        } else {
            Err(if stderr.is_empty() {
                format!("sftp ls exited with status: {}", output.status)
            } else {
                stderr
            })
        }
    }

    /// Cancel an active upload by killing its subprocess.
    pub fn cancel(&self, session_id: &str) -> Result<(), String> {
        let mut processes = self.processes.lock().unwrap();
        if let Some(mut child) = processes.remove(session_id) {
            child
                .kill()
                .map_err(|e| format!("Failed to kill sftp process: {}", e))?;
            Ok(())
        } else {
            Err(format!("No active upload for session: {}", session_id))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sftp_process_manager_new() {
        let manager = SftpProcessManager::new();
        // Should not panic
        let processes = manager.processes.lock().unwrap();
        assert!(processes.is_empty());
    }

    #[test]
    fn test_cancel_nonexistent_session() {
        let manager = SftpProcessManager::new();
        let result = manager.cancel("nonexistent");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("No active upload"));
    }
}
