//! SSH binary detection utilities.
//!
//! Detects the openssh binary path on supported platforms (Linux, Windows).

use serde::Deserialize;
use std::path::PathBuf;

/// Argument struct for build_ssh_args Tauri command deserialization.
#[derive(Debug, Deserialize)]
pub struct SshOptionArg {
    pub key: String,
    pub value: String,
}

/// Detects the SSH binary path on the current platform.
///
/// # Platform Behavior
///
/// - **Linux**: Searches PATH for `ssh`
/// - **Windows**: Checks `C:\Windows\System32\OpenSSH\ssh.exe` first, then searches PATH
///
/// # Returns
///
/// Full path to the ssh binary, or empty string if not found.
pub fn detect_ssh_command() -> String {
    #[cfg(unix)]
    {
        detect_ssh_unix()
    }

    #[cfg(windows)]
    {
        detect_ssh_windows()
    }
}

#[cfg(unix)]
fn detect_ssh_unix() -> String {
    if let Ok(path) = which("ssh") {
        path.to_string_lossy().to_string()
    } else {
        String::new()
    }
}

#[cfg(windows)]
fn detect_ssh_windows() -> String {
    // Check System32 OpenSSH first
    let system32_path = PathBuf::from(r"C:\Windows\System32\OpenSSH\ssh.exe");
    if system32_path.is_file() {
        return system32_path.to_string_lossy().to_string();
    }

    // Fall back to PATH search
    if let Ok(path) = which("ssh.exe") {
        path.to_string_lossy().to_string()
    } else {
        String::new()
    }
}

/// Search PATH for a binary by name.
fn which(name: &str) -> Result<PathBuf, ()> {
    let path_var = std::env::var("PATH").map_err(|_| ())?;

    #[cfg(unix)]
    let separator = ':';
    #[cfg(windows)]
    let separator = ';';

    for dir in path_var.split(separator) {
        let candidate = PathBuf::from(dir).join(name);
        if candidate.is_file() {
            return Ok(candidate);
        }
    }
    Err(())
}

/// Build SSH command arguments from connection settings.
///
/// Returns a Vec of arguments to pass to the ssh binary.
/// ssh_options are converted to -o Key=Value arguments.
pub fn build_ssh_args(
    hostname: &str,
    port: u16,
    username: &str,
    identity_file: &str,
    ssh_options: &[(String, String)],
) -> Vec<String> {
    let mut args = Vec::new();

    if port != 22 {
        args.push("-p".to_string());
        args.push(port.to_string());
    }

    if !identity_file.is_empty() {
        let expanded = expand_tilde(identity_file);
        args.push("-i".to_string());
        args.push(expanded);
    }

    for (key, value) in ssh_options {
        if !key.is_empty() {
            args.push("-o".to_string());
            args.push(format!("{}={}", key, value));
        }
    }

    if !username.is_empty() {
        args.push(format!("{}@{}", username, hostname));
    } else {
        args.push(hostname.to_string());
    }

    args
}

/// Expand `~` prefix to the user's home directory.
pub fn expand_tilde(path: &str) -> String {
    let Some(rest) = path.strip_prefix('~') else {
        return path.to_string();
    };

    let Some(home) = super::home_dir() else {
        return path.to_string();
    };

    if rest.is_empty() {
        home
    } else if rest.starts_with('/') || rest.starts_with('\\') {
        format!("{}{}", home, rest)
    } else {
        // ~otheruser - not expanded
        path.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detect_ssh_command_returns_string() {
        let result = detect_ssh_command();
        // On most systems, ssh is installed
        // Just check it doesn't panic
        let _ = result;
    }

    #[cfg(unix)]
    #[test]
    fn test_detect_ssh_on_linux() {
        let result = detect_ssh_command();
        // On CI/test systems, ssh is typically installed
        if !result.is_empty() {
            assert!(
                result.contains("ssh"),
                "Result should contain 'ssh': {}",
                result
            );
        }
    }

    #[test]
    fn test_which_nonexistent_binary() {
        let result = which("this_binary_does_not_exist_12345");
        assert!(result.is_err());
    }

    #[cfg(unix)]
    #[test]
    fn test_which_finds_common_binary() {
        // `sh` should exist on every Unix system
        let result = which("sh");
        assert!(result.is_ok());
    }

    // -- build_ssh_args tests --

    #[test]
    fn test_build_ssh_args_minimal() {
        let args = build_ssh_args("example.com", 22, "", "", &[]);
        assert_eq!(args, vec!["example.com"]);
    }

    #[test]
    fn test_build_ssh_args_with_all_fields() {
        let opts = vec![("StrictHostKeyChecking".to_string(), "no".to_string())];
        let args = build_ssh_args("example.com", 2222, "user", "~/.ssh/id_rsa", &opts);
        assert_eq!(args[0], "-p");
        assert_eq!(args[1], "2222");
        assert_eq!(args[2], "-i");
        // identity_file is expanded
        assert!(args[3].ends_with("/.ssh/id_rsa") || args[3].contains(".ssh"));
        assert_eq!(args[4], "-o");
        assert_eq!(args[5], "StrictHostKeyChecking=no");
        assert_eq!(args[6], "user@example.com");
    }

    #[test]
    fn test_build_ssh_args_with_custom_port() {
        let args = build_ssh_args("host.com", 8022, "", "", &[]);
        assert_eq!(args, vec!["-p", "8022", "host.com"]);
    }

    #[test]
    fn test_build_ssh_args_with_username() {
        let args = build_ssh_args("host.com", 22, "admin", "", &[]);
        assert_eq!(args, vec!["admin@host.com"]);
    }

    #[test]
    fn test_build_ssh_args_with_identity_file() {
        let args = build_ssh_args("host.com", 22, "", "/path/to/key", &[]);
        assert_eq!(args, vec!["-i", "/path/to/key", "host.com"]);
    }

    #[test]
    fn test_build_ssh_args_with_ssh_options() {
        let opts = vec![
            ("StrictHostKeyChecking".to_string(), "no".to_string()),
            ("ServerAliveInterval".to_string(), "60".to_string()),
        ];
        let args = build_ssh_args("host.com", 22, "", "", &opts);
        assert_eq!(
            args,
            vec![
                "-o",
                "StrictHostKeyChecking=no",
                "-o",
                "ServerAliveInterval=60",
                "host.com"
            ]
        );
    }

    #[test]
    fn test_build_ssh_args_skips_empty_key() {
        let opts = vec![("".to_string(), "value".to_string())];
        let args = build_ssh_args("host.com", 22, "", "", &opts);
        assert_eq!(args, vec!["host.com"]);
    }

    // -- expand_tilde tests --

    #[test]
    fn test_expand_tilde_no_tilde() {
        assert_eq!(expand_tilde("/absolute/path"), "/absolute/path");
    }

    #[test]
    fn test_expand_tilde_with_home() {
        let result = expand_tilde("~/.ssh/id_rsa");
        assert!(!result.starts_with('~'), "Should expand ~: {}", result);
        assert!(result.ends_with("/.ssh/id_rsa"));
    }

    #[test]
    fn test_expand_tilde_just_tilde() {
        let result = expand_tilde("~");
        assert!(!result.starts_with('~') || result == "~");
    }

    #[test]
    fn test_expand_tilde_other_user() {
        // ~otheruser should not be expanded
        assert_eq!(expand_tilde("~otheruser/.ssh"), "~otheruser/.ssh");
    }
}
