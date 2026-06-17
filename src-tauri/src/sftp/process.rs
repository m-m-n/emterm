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

/// Escape a value for safe interpolation inside an sftp batch command's
/// double-quoted argument.
///
/// sftp batch lines (`-b -`) treat backslash as an escape and double-quote as
/// the argument delimiter, so an unescaped `\` or `"` in an interpolated path
/// can terminate the argument early or smuggle further tokens. This is applied
/// at emission time so callers need not pre-validate the value; in particular it
/// neutralizes a Windows trailing backslash (`C:\dir\` → `C:\\dir\\`) that would
/// otherwise escape the closing quote.
fn escape_sftp_arg(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for c in value.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            other => out.push(other),
        }
    }
    out
}

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
    // Mirrors the source `commands/sftp.rs` signature (connection fields +
    // paths) one-to-one to preserve port parity; grouping into a struct would
    // diverge from the ported call sites.
    #[allow(clippy::too_many_arguments)]
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
            // Batch-mode (`-b -`) echoes the commands on stdout; we never read
            // it, so discard it. Piping stdout without draining it would
            // deadlock the child once the pipe buffer fills on a large upload.
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| format!("Failed to spawn sftp: {}", e))?;

        // Write put command to stdin. Paths are escaped at emission time so an
        // embedded `"` / `\` (incl. a Windows trailing backslash) cannot break
        // out of the quoted argument.
        let local = escape_sftp_arg(local_path);
        let remote = escape_sftp_arg(remote_path);
        let put_cmd = if is_directory {
            format!("put -r \"{}\" \"{}\"\nbye\n", local, remote)
        } else {
            format!("put \"{}\" \"{}\"\nbye\n", local, remote)
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
            for line in reader.lines().map_while(Result::ok) {
                if let Some(error_msg) = parse_error_line(&line) {
                    stderr_errors.push(error_msg);
                }
            }
        }

        // Remove the child from the registry *before* waiting, so the
        // `processes` mutex is not held across the (potentially long) wait —
        // otherwise a UI-thread `cancel()` would block until the upload
        // finishes. A concurrent `cancel()` that wins the race removes (and
        // kills) the child first, leaving `None` here → "Upload cancelled".
        let child = { self.processes.lock().unwrap().remove(session_id) };
        if let Some(mut child) = child {
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
    // Mirrors the source connection-field signature to preserve port parity.
    #[allow(clippy::too_many_arguments)]
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

        // Write ls command to stdin (path escaped at emission time).
        let ls_cmd = format!("ls \"{}\"\nbye\n", escape_sftp_arg(remote_dir));

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

impl Drop for SftpProcessManager {
    fn drop(&mut self) {
        let mut processes = self.processes.lock().unwrap_or_else(|e| e.into_inner());
        for (_, mut child) in processes.drain() {
            if let Err(e) = child.kill() {
                log::warn!("Failed to kill sftp process on drop: {}", e);
            }
            let _ = child.wait();
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

    #[test]
    fn escape_sftp_arg_passes_through_plain() {
        assert_eq!(
            escape_sftp_arg("/home/user/file.txt"),
            "/home/user/file.txt"
        );
        assert_eq!(
            escape_sftp_arg("dir name with spaces"),
            "dir name with spaces"
        );
    }

    #[test]
    fn escape_sftp_arg_escapes_quote_and_backslash() {
        // A double-quote would otherwise terminate the quoted argument.
        assert_eq!(escape_sftp_arg("a\"b"), "a\\\"b");
        // A backslash is doubled so it can't escape the following character.
        assert_eq!(escape_sftp_arg("a\\b"), "a\\\\b");
        // Both together.
        assert_eq!(escape_sftp_arg("a\\\"b"), "a\\\\\\\"b");
    }

    #[test]
    fn escape_sftp_arg_neutralizes_windows_trailing_backslash() {
        // `C:\dir\` — the trailing backslash must not escape the closing quote
        // when interpolated into `put "{}" "..."`.
        assert_eq!(escape_sftp_arg("C:\\dir\\"), "C:\\\\dir\\\\");
        // Embedded in the emitted command, the closing quote stays intact.
        let emitted = format!("put \"{}\" \"/remote\"\n", escape_sftp_arg("C:\\dir\\"));
        assert!(emitted.contains("put \"C:\\\\dir\\\\\" \"/remote\"\n"));
    }

    #[test]
    fn escape_sftp_arg_blocks_command_injection_via_quote_break() {
        // Without escaping, `x" ; rm -rf /; "` would break out of the argument.
        let escaped = escape_sftp_arg("x\" ; rm -rf /; \"");
        // No bare double-quote remains; every `"` is preceded by a backslash.
        let bytes: Vec<char> = escaped.chars().collect();
        for (i, c) in bytes.iter().enumerate() {
            if *c == '"' {
                assert!(i > 0 && bytes[i - 1] == '\\', "bare quote at {}", i);
            }
        }
    }
}
