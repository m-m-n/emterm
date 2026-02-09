use std::process::Command;

/// Check if a file exists at the given path.
///
/// Returns true only for regular files (not directories, symlinks to directories, etc.).
#[tauri::command]
pub fn check_file_exists(path: String) -> Result<bool, String> {
    Ok(std::path::Path::new(&path).is_file())
}

/// Open a file in an external editor.
///
/// Receives the program name and arguments as separate values
/// to avoid shell interpretation and prevent command injection.
/// The spawned child process is reaped in a background thread
/// to prevent zombie process accumulation.
#[tauri::command]
pub fn open_file_in_editor(program: String, args: Vec<String>) -> Result<(), String> {
    if program.is_empty() {
        return Err("Editor program name is empty".to_string());
    }

    let mut child = Command::new(&program)
        .args(&args)
        .spawn()
        .map_err(|e| format!("Failed to launch editor '{}': {}", program, e))?;

    // Reap child in background to prevent zombie process accumulation
    std::thread::spawn(move || {
        let _ = child.wait();
    });

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_check_file_exists_for_existing_file() {
        // Cargo.toml always exists in our project
        let result = check_file_exists("Cargo.toml".to_string());
        assert!(result.is_ok());
        assert!(result.unwrap());
    }

    #[test]
    fn test_check_file_exists_for_non_existing_file() {
        let result = check_file_exists("/nonexistent/path/to/file.txt".to_string());
        assert!(result.is_ok());
        assert!(!result.unwrap());
    }

    #[test]
    fn test_check_file_exists_returns_false_for_directory() {
        let result = check_file_exists("/tmp".to_string());
        assert!(result.is_ok());
        assert!(!result.unwrap());
    }

    #[test]
    fn test_open_file_in_editor_rejects_empty_program() {
        let result = open_file_in_editor("".to_string(), vec![]);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("empty"));
    }
}
