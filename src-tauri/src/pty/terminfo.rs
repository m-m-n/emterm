//! Custom terminfo installation for eMterm.
//!
//! Installs `emterm-256color` terminfo entry to `~/.terminfo/` on first use.
//! Based on `xterm-256color` with `ccc` and `initc` removed to prevent
//! ncurses from using explicit black (SGR 30) instead of default foreground
//! (SGR 39) for UI elements like borders.

use std::path::PathBuf;
use std::sync::OnceLock;

/// The TERM value to use when emterm-256color is available.
pub const EMTERM_TERM: &str = "emterm-256color";

/// Fallback TERM value when custom terminfo is not available.
const FALLBACK_TERM: &str = "xterm-256color";

/// Terminfo source: inherits xterm-256color, removes ccc and initc.
///
/// Removing `ccc` (can_change_color) and `initc` (initialize_color) prevents
/// ncurses from treating the terminal as having a mutable palette. This causes
/// ncurses apps (e.g. glances, nethogs) to use default foreground color (SGR 39)
/// instead of explicit black (SGR 30) for borders and bars.
const TERMINFO_SOURCE: &str = "\
emterm-256color|eMterm terminal emulator with 256 colors,\n\
\tuse=xterm-256color,\n\
\tccc@, initc@,\n";

/// Cached result of terminfo installation.
static INSTALLED: OnceLock<bool> = OnceLock::new();

/// Get the home directory from environment variables.
fn home_dir() -> Option<PathBuf> {
    #[cfg(unix)]
    {
        std::env::var_os("HOME").map(PathBuf::from)
    }
    #[cfg(windows)]
    {
        std::env::var_os("USERPROFILE").map(PathBuf::from)
    }
}

/// Get the appropriate TERM value, installing custom terminfo if needed.
///
/// Returns `emterm-256color` if the custom terminfo is available (or was
/// successfully installed), otherwise falls back to `xterm-256color`.
#[cfg(unix)]
pub fn get_term() -> &'static str {
    let installed = INSTALLED.get_or_init(|| {
        if is_installed() {
            return true;
        }
        match install() {
            Ok(()) => {
                log::info!("Installed emterm-256color terminfo");
                true
            }
            Err(e) => {
                log::warn!("Failed to install emterm-256color terminfo: {}. Falling back to xterm-256color", e);
                false
            }
        }
    });
    if *installed { EMTERM_TERM } else { FALLBACK_TERM }
}

#[cfg(windows)]
pub fn get_term() -> &'static str {
    FALLBACK_TERM
}

/// Check if emterm-256color is already installed in ~/.terminfo/.
fn is_installed() -> bool {
    if let Some(path) = terminfo_path() {
        path.exists()
    } else {
        false
    }
}

/// Get the expected path for the compiled terminfo entry.
fn terminfo_path() -> Option<PathBuf> {
    home_dir().map(|home| {
        home.join(".terminfo")
            .join("e")
            .join("emterm-256color")
    })
}

/// Install the custom terminfo entry using `tic`.
#[cfg(unix)]
fn install() -> Result<(), String> {
    let tmp_dir = std::env::temp_dir();
    // Use PID-unique filename to prevent symlink attacks and cross-process races
    let src_path = tmp_dir.join(format!("emterm-{}.ti", std::process::id()));

    // Write terminfo source to temp file
    std::fs::write(&src_path, TERMINFO_SOURCE)
        .map_err(|e| format!("Failed to write terminfo source: {}", e))?;

    // Compile with tic into ~/.terminfo
    let home = home_dir()
        .ok_or_else(|| "Could not determine home directory".to_string())?;
    let terminfo_dir = home.join(".terminfo");

    let output = std::process::Command::new("tic")
        .args(["-x", "-o"])
        .arg(&terminfo_dir)
        .arg(&src_path)
        .output()
        .map_err(|e| format!("Failed to run tic: {}", e))?;

    // Clean up temp file (best-effort)
    let _ = std::fs::remove_file(&src_path);

    if output.status.success() {
        Ok(())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        Err(format!("tic failed: {}", stderr.trim()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_terminfo_source_is_valid() {
        assert!(TERMINFO_SOURCE.contains("emterm-256color"));
        assert!(TERMINFO_SOURCE.contains("use=xterm-256color"));
        assert!(TERMINFO_SOURCE.contains("ccc@"));
        assert!(TERMINFO_SOURCE.contains("initc@"));
    }

    #[test]
    fn test_terminfo_path() {
        let path = terminfo_path();
        assert!(path.is_some());
        let path = path.unwrap();
        assert!(path.ends_with(".terminfo/e/emterm-256color"));
    }

    #[test]
    fn test_get_term_returns_valid_value() {
        let term = get_term();
        assert!(
            term == EMTERM_TERM || term == FALLBACK_TERM,
            "TERM should be emterm-256color or xterm-256color, got: {}", term
        );
    }
}
