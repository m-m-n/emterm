/// Status bar command execution.
use crate::ssh::detect::expand_tilde;

/// Allowed built-in programs for status bar providers.
const ALLOWED_PROGRAMS: &[&str] = &["git"];

/// PE executable magic bytes (`MZ`).
#[cfg(any(windows, test))]
const PE_MAGIC: [u8; 2] = [0x4D, 0x5A];

/// Maximum bytes to read when looking for a shebang line.
#[cfg(any(windows, test))]
const SHEBANG_MAX_READ: usize = 256;

/// Result of resolving a Windows executable file.
#[cfg(any(windows, test))]
#[derive(Debug, PartialEq)]
enum WindowsExecutable {
    /// PE executable - run directly.
    Direct(String),
    /// Script with shebang - run interpreter with script path as argument.
    Interpreted { interpreter: String, script: String },
}

/// Check if a file is a PE executable by reading its first 2 bytes.
#[cfg(any(windows, test))]
fn is_pe_file(path: &str) -> std::io::Result<bool> {
    use std::io::Read;
    let mut file = std::fs::File::open(path)?;
    let mut magic = [0u8; 2];
    let bytes_read = file.read(&mut magic)?;
    Ok(bytes_read == 2 && magic == PE_MAGIC)
}

/// Parse a shebang line from a file, returning the interpreter path.
///
/// Reads up to `SHEBANG_MAX_READ` bytes and extracts the interpreter
/// from the first line if it starts with `#!`.
#[cfg(any(windows, test))]
fn parse_shebang(path: &str) -> Result<String, String> {
    use std::io::Read;
    let mut file =
        std::fs::File::open(path).map_err(|e| format!("Cannot read '{}': {}", path, e))?;
    let mut buf = [0u8; SHEBANG_MAX_READ];
    let bytes_read = file
        .read(&mut buf)
        .map_err(|e| format!("Cannot read '{}': {}", path, e))?;
    if bytes_read == 0 {
        return Err(format!("No shebang found in script file: {}", path));
    }
    let buf = &buf[..bytes_read];

    // Extract first line (up to \n or \r\n)
    let first_line_end = buf.iter().position(|&b| b == b'\n').unwrap_or(bytes_read);
    let first_line = &buf[..first_line_end];

    // Check for shebang prefix
    if first_line.len() < 2 || first_line[0] != b'#' || first_line[1] != b'!' {
        return Err(format!("No shebang found in script file: {}", path));
    }

    // Extract interpreter path: strip `#!`, trim whitespace, strip trailing \r
    let interpreter = std::str::from_utf8(&first_line[2..])
        .map_err(|_| format!("Shebang line is not valid UTF-8 in: {}", path))?
        .trim();

    if interpreter.is_empty() {
        return Err(format!("Empty interpreter path in shebang: {}", path));
    }

    Ok(interpreter.to_string())
}

/// Resolve how to execute a file on Windows.
///
/// PE files are executed directly. Script files are parsed for a shebang
/// line to determine the interpreter.
#[cfg(any(windows, test))]
fn resolve_windows_executable(path: &str) -> Result<WindowsExecutable, String> {
    match is_pe_file(path) {
        Ok(true) => Ok(WindowsExecutable::Direct(path.to_string())),
        Ok(false) => {
            let interpreter = parse_shebang(path)?;
            Ok(WindowsExecutable::Interpreted {
                interpreter,
                script: path.to_string(),
            })
        }
        Err(e) => Err(format!("Cannot read '{}': {}", path, e)),
    }
}

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

    // Expand ~ to home directory
    let program_expanded = expand_tilde(program_trimmed);

    // On Windows, resolve PE vs script before allowlist check
    #[cfg(windows)]
    let (effective_program, effective_args) = {
        // Try to resolve PE vs script; NotFound means a PATH-based command (e.g., "git")
        let resolve_result = match std::fs::metadata(&program_expanded) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
            Err(e) => {
                return Err(format!("Cannot access '{}': {}", program_trimmed, e));
            }
            Ok(_) => Some(resolve_windows_executable(&program_expanded)),
        };

        match resolve_result {
            Some(Ok(WindowsExecutable::Direct(path))) => {
                // PE file: allowlist check on the executable itself
                if !is_program_allowed(&app, &path) {
                    return Err(format!(
                        "Program '{}' is not allowed. Only built-in programs and configured custom commands are permitted.",
                        program_trimmed
                    ));
                }
                (path, args)
            }
            Some(Ok(WindowsExecutable::Interpreted {
                interpreter,
                script,
            })) => {
                // Script file: allowlist check on the script (the user-registered path),
                // interpreter is auto-trusted (FR5)
                if !is_program_allowed(&app, &script) {
                    return Err(format!(
                        "Program '{}' is not allowed. Only built-in programs and configured custom commands are permitted.",
                        program_trimmed
                    ));
                }
                let mut new_args = vec![script];
                new_args.extend(args);
                (interpreter, new_args)
            }
            Some(Err(e)) => {
                return Err(e);
            }
            None => {
                // File not found: PATH-based command (e.g., "git")
                if !is_program_allowed(&app, &program_expanded) {
                    return Err(format!(
                        "Program '{}' is not allowed. Only built-in programs and configured custom commands are permitted.",
                        program_trimmed
                    ));
                }
                (program_expanded.clone(), args)
            }
        }
    };

    #[cfg(not(windows))]
    {
        // Validate program against allowlist (use expanded path for consistent comparison)
        if !is_program_allowed(&app, &program_expanded) {
            return Err(format!(
                "Program '{}' is not allowed. Only built-in programs and configured custom commands are permitted.",
                program_trimmed
            ));
        }
    }

    #[cfg(not(windows))]
    let (effective_program, effective_args) = (program_expanded.clone(), args);

    let mut cmd = std::process::Command::new(&effective_program);
    cmd.args(&effective_args)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());

    if !cwd.is_empty() {
        cmd.current_dir(&cwd);
    }

    // FR6: Prevent console window flash on Windows
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x08000000;
        cmd.creation_flags(CREATE_NO_WINDOW);
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
    // Compare with tilde-expanded paths so ~/foo matches /home/user/foo
    if let Ok(settings) = super::config::load_settings(app.clone()) {
        for cmd in settings.statusbar_custom_commands.values() {
            let exe_expanded = expand_tilde(&cmd.executable);
            if exe_expanded == program {
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

#[cfg(test)]
mod shebang_tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    fn create_temp_file(content: &[u8]) -> NamedTempFile {
        let mut f = NamedTempFile::new().unwrap();
        f.write_all(content).unwrap();
        f.flush().unwrap();
        f
    }

    // --- is_pe_file tests ---

    #[test]
    fn test_pe_file_detected() {
        let f = create_temp_file(&[0x4D, 0x5A, 0x90, 0x00]);
        assert!(is_pe_file(f.path().to_str().unwrap()).unwrap());
    }

    #[test]
    fn test_non_pe_file() {
        let f = create_temp_file(b"#!/usr/bin/env python\nprint('hi')");
        assert!(!is_pe_file(f.path().to_str().unwrap()).unwrap());
    }

    #[test]
    fn test_pe_check_empty_file() {
        let f = create_temp_file(b"");
        assert!(!is_pe_file(f.path().to_str().unwrap()).unwrap());
    }

    #[test]
    fn test_pe_check_one_byte() {
        let f = create_temp_file(&[0x4D]);
        assert!(!is_pe_file(f.path().to_str().unwrap()).unwrap());
    }

    #[test]
    fn test_pe_check_nonexistent_file() {
        assert!(is_pe_file("/tmp/nonexistent_file_12345").is_err());
    }

    // --- parse_shebang tests ---

    #[test]
    fn test_shebang_simple_path() {
        let f = create_temp_file(b"#!/usr/bin/python\nimport sys\n");
        let result = parse_shebang(f.path().to_str().unwrap()).unwrap();
        assert_eq!(result, "/usr/bin/python");
    }

    #[test]
    fn test_shebang_windows_path() {
        let f = create_temp_file(b"#!C:\\Python\\python.exe\r\nimport sys\r\n");
        let result = parse_shebang(f.path().to_str().unwrap()).unwrap();
        assert_eq!(result, "C:\\Python\\python.exe");
    }

    #[test]
    fn test_shebang_with_extra_whitespace() {
        let f = create_temp_file(b"#!  C:\\Python\\python.exe  \nprint('hi')");
        let result = parse_shebang(f.path().to_str().unwrap()).unwrap();
        assert_eq!(result, "C:\\Python\\python.exe");
    }

    #[test]
    fn test_shebang_env_style() {
        let f = create_temp_file(b"#!/usr/bin/env bun\nconsole.log('hi')");
        let result = parse_shebang(f.path().to_str().unwrap()).unwrap();
        assert_eq!(result, "/usr/bin/env bun");
    }

    #[test]
    fn test_no_shebang_returns_error() {
        let f = create_temp_file(b"print('hello')\n");
        let result = parse_shebang(f.path().to_str().unwrap());
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("No shebang found"));
    }

    #[test]
    fn test_empty_file_returns_error() {
        let f = create_temp_file(b"");
        let result = parse_shebang(f.path().to_str().unwrap());
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("No shebang found"));
    }

    #[test]
    fn test_shebang_empty_interpreter_returns_error() {
        let f = create_temp_file(b"#!\nsome code");
        let result = parse_shebang(f.path().to_str().unwrap());
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Empty interpreter path"));
    }

    #[test]
    fn test_shebang_only_whitespace_after_hash_bang() {
        let f = create_temp_file(b"#!   \nsome code");
        let result = parse_shebang(f.path().to_str().unwrap());
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Empty interpreter path"));
    }

    #[test]
    fn test_binary_file_no_shebang() {
        let f = create_temp_file(&[0x00, 0xFF, 0xFE, 0x01, 0x02]);
        let result = parse_shebang(f.path().to_str().unwrap());
        assert!(result.is_err());
    }

    #[test]
    fn test_long_first_line_no_newline() {
        // Line longer than SHEBANG_MAX_READ with no newline
        let mut content = b"#!/very/long/path/".to_vec();
        content.extend(std::iter::repeat(b'x').take(300));
        let f = create_temp_file(&content);
        // Should still parse (truncated at SHEBANG_MAX_READ)
        let result = parse_shebang(f.path().to_str().unwrap()).unwrap();
        assert!(result.starts_with("/very/long/path/"));
    }

    #[test]
    fn test_nonexistent_file_returns_error() {
        let result = parse_shebang("/tmp/nonexistent_shebang_test_12345");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Cannot read"));
    }

    // --- resolve_windows_executable tests ---

    #[test]
    fn test_resolve_pe_returns_direct() {
        let f = create_temp_file(&[0x4D, 0x5A, 0x90, 0x00]);
        let path = f.path().to_str().unwrap();
        let result = resolve_windows_executable(path).unwrap();
        assert_eq!(result, WindowsExecutable::Direct(path.to_string()));
    }

    #[test]
    fn test_resolve_script_returns_interpreted() {
        let f = create_temp_file(b"#!/usr/bin/python\nimport sys\n");
        let path = f.path().to_str().unwrap();
        let result = resolve_windows_executable(path).unwrap();
        assert_eq!(
            result,
            WindowsExecutable::Interpreted {
                interpreter: "/usr/bin/python".to_string(),
                script: path.to_string(),
            }
        );
    }

    #[test]
    fn test_resolve_no_shebang_returns_error() {
        let f = create_temp_file(b"just some text\n");
        let path = f.path().to_str().unwrap();
        let result = resolve_windows_executable(path);
        assert!(result.is_err());
    }

    #[test]
    fn test_resolve_nonexistent_returns_error() {
        let result = resolve_windows_executable("/tmp/nonexistent_resolve_test_12345");
        assert!(result.is_err());
    }
}
