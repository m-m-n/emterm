/// Status bar command execution.

/// Allowed built-in programs for status bar providers.
const ALLOWED_PROGRAMS: &[&str] = &["git"];

/// Run a command with arguments and working directory.
///
/// Security: Only allows built-in programs (git) and user-configured
/// custom command executables from settings. Rejects all other programs.
#[cfg(feature = "gui")]
#[tauri::command]
pub fn run_statusbar_shell_command(
    app: tauri::AppHandle,
    program: String,
    args: Vec<String>,
    cwd: String,
) -> Result<String, String> {
    let program_trimmed = program.trim();
    if program_trimmed.is_empty() {
        return Err("Program path is empty".into());
    }

    // Validate program against allowlist
    if !is_program_allowed(&app, program_trimmed) {
        return Err(format!(
            "Program '{}' is not allowed. Only built-in programs and configured custom commands are permitted.",
            program_trimmed
        ));
    }

    let mut cmd = std::process::Command::new(program_trimmed);
    cmd.args(&args)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());

    if !cwd.is_empty() {
        cmd.current_dir(&cwd);
    }

    let output = cmd
        .output()
        .map_err(|e| format!("Failed to execute '{}': {}", program_trimmed, e))?;

    // For git commands, non-zero exit is expected (e.g., not a git repo)
    // Return stdout regardless, let the caller decide
    String::from_utf8(output.stdout)
        .map_err(|e| format!("Command output is not valid UTF-8: {}", e))
}

/// Check if a program is in the allowlist (built-in or configured custom command).
#[cfg(feature = "gui")]
fn is_program_allowed(app: &tauri::AppHandle, program: &str) -> bool {
    // Check built-in allowed programs
    if ALLOWED_PROGRAMS.contains(&program) {
        return true;
    }

    // Check user-configured custom command executables from settings
    if let Ok(settings) = super::config::load_settings(app.clone()) {
        for cmd in settings.statusbar_custom_commands.values() {
            if cmd.executable == program {
                return true;
            }
        }
    }

    false
}

#[cfg(all(test, feature = "gui"))]
mod tests {
    use super::*;

    #[test]
    fn test_allowed_programs_contains_git() {
        assert!(ALLOWED_PROGRAMS.contains(&"git"));
    }

    #[test]
    fn test_disallowed_program() {
        // Without AppHandle we can't test is_program_allowed directly,
        // but we can verify the constant list
        assert!(!ALLOWED_PROGRAMS.contains(&"rm"));
        assert!(!ALLOWED_PROGRAMS.contains(&"curl"));
        assert!(!ALLOWED_PROGRAMS.contains(&"sh"));
    }
}
