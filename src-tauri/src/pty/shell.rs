//! Shell detection utilities for cross-platform default shell discovery.
//!
//! This module provides functionality to detect the default shell on
//! different platforms (Linux, macOS, Windows).

/// Detects the default shell for the current platform.
///
/// # Platform Behavior
///
/// - **Linux**: Returns `$SHELL` environment variable, or `/bin/sh` as fallback
/// - **macOS**: Returns `$SHELL` environment variable, or `/bin/zsh` as fallback
/// - **Windows**: Returns `powershell.exe`
///
/// # Returns
///
/// A string containing the path or name of the default shell.
///
/// # Examples
///
/// ```ignore
/// use app_lib::pty::detect_default_shell;
///
/// let shell = detect_default_shell();
/// assert!(!shell.is_empty());
/// ```
pub fn detect_default_shell() -> String {
    #[cfg(unix)]
    {
        std::env::var("SHELL").unwrap_or_else(|_| {
            #[cfg(target_os = "macos")]
            {
                "/bin/zsh".to_string()
            }
            #[cfg(not(target_os = "macos"))]
            {
                "/bin/sh".to_string()
            }
        })
    }

    #[cfg(windows)]
    {
        "powershell.exe".to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detect_default_shell_returns_non_empty() {
        let shell = detect_default_shell();
        assert!(!shell.is_empty(), "Shell path should not be empty");
    }

    #[cfg(unix)]
    #[test]
    fn test_detect_default_shell_returns_valid_path() {
        let shell = detect_default_shell();
        // On Unix, the shell should be an absolute path or a command name
        assert!(
            shell.starts_with('/') || !shell.contains('/'),
            "Shell should be an absolute path or command name: {}",
            shell
        );
    }

    #[cfg(windows)]
    #[test]
    fn test_detect_default_shell_windows() {
        let shell = detect_default_shell();
        assert_eq!(shell, "powershell.exe");
    }
}
