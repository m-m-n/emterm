//! Windows-only helpers for resolving and spawning user-configured
//! executables (status-bar custom commands, mux statusbar commands).
//!
//! Lifted out of `status_bar::providers::worker` so the mux IPC layer
//! and the status-bar provider layer can both depend on a shared
//! platform module rather than the status-bar layer (mux currently
//! reaching across subsystems into a sibling).
//!
//! `cfg(any(windows, test))`: unit tests for the resolver/shebang
//! parser need to compile on Linux CI; the real spawn-time use of
//! `resolve_for_windows` / `CREATE_NO_WINDOW` stays Windows-only.

use std::io::Read;

/// PE executable magic bytes (`MZ`).
pub(crate) const PE_MAGIC: [u8; 2] = [0x4D, 0x5A];

/// Maximum bytes to read when looking for a shebang line.
pub(crate) const SHEBANG_MAX_READ: usize = 256;

/// Windows: prevent a transient console window from flashing every time
/// a child process is spawned from a GUI-subsystem parent (e.g. `git.exe`).
#[cfg(windows)]
pub const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// Outcome of [`resolve_windows_executable`].
#[derive(Debug, PartialEq, Eq)]
pub enum WindowsExecutable {
    /// PE executable — run directly.
    Direct(String),
    /// Shebang script — run the interpreter with `interpreter_args`
    /// followed by the script path as the trailing arguments.
    Interpreted {
        interpreter: String,
        interpreter_args: Vec<String>,
        script: String,
    },
}

fn is_pe_file(path: &str) -> std::io::Result<bool> {
    let mut file = std::fs::File::open(path)?;
    let mut magic = [0u8; 2];
    let bytes_read = file.read(&mut magic)?;
    Ok(bytes_read == 2 && magic == PE_MAGIC)
}

/// Parse a shebang line into `(interpreter, extra_args)`.
///
/// `#!/usr/bin/env python3` → `("/usr/bin/env", ["python3"])`.
/// `#!/usr/bin/python -u`   → `("/usr/bin/python", ["-u"])`.
/// Returns `Err` if the file lacks a `#!` prefix or the shebang is empty.
fn parse_shebang(path: &str) -> Result<(String, Vec<String>), String> {
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
    let first_line_end = buf.iter().position(|&b| b == b'\n').unwrap_or(bytes_read);
    let first_line = &buf[..first_line_end];
    if first_line.len() < 2 || first_line[0] != b'#' || first_line[1] != b'!' {
        return Err(format!("No shebang found in script file: {}", path));
    }
    let body = std::str::from_utf8(&first_line[2..])
        .map_err(|_| format!("Shebang line is not valid UTF-8 in: {}", path))?
        .trim();
    let mut parts = body.split_whitespace();
    let interpreter = parts
        .next()
        .ok_or_else(|| format!("Empty interpreter path in shebang: {}", path))?
        .to_string();
    let extra: Vec<String> = parts.map(str::to_string).collect();
    Ok((interpreter, extra))
}

/// `/usr/bin/env python3` → `("python3", [])`. Windows has no
/// `/usr/bin/env`, so when the shebang interpreter is `env` (any path
/// ending in `/env` or `\env`, with optional `.exe`), strip it and use
/// the first non-flag argument as the new interpreter; the remaining
/// args are kept. `env`-style `NAME=VALUE` and `-u`/`-S` flags are
/// skipped over until a real interpreter name is found.
fn normalize_env_shebang(
    interpreter: String,
    interpreter_args: Vec<String>,
) -> (String, Vec<String>) {
    let trimmed = interpreter
        .strip_suffix(".exe")
        .or_else(|| interpreter.strip_suffix(".EXE"))
        .unwrap_or(&interpreter);
    let basename = trimmed.rsplit(['/', '\\']).next().unwrap_or(trimmed);
    if basename != "env" {
        return (interpreter, interpreter_args);
    }
    let mut rest = interpreter_args.into_iter().peekable();
    while let Some(arg) = rest.peek() {
        // Skip env flags (`-S`, `-u`, etc.) and `NAME=VALUE` env-var
        // assignments; stop at the first plain token, which is the
        // real interpreter name.
        if arg.starts_with('-') || arg.contains('=') {
            rest.next();
        } else {
            break;
        }
    }
    match rest.next() {
        Some(new_interp) => (new_interp, rest.collect()),
        None => (interpreter, Vec::new()),
    }
}

/// Resolve how to execute `path`. PE files run directly; shebang-prefixed
/// scripts route through the interpreter named in the shebang line,
/// including any interpreter args parsed from the same line.
pub fn resolve_windows_executable(path: &str) -> Result<WindowsExecutable, String> {
    match is_pe_file(path) {
        Ok(true) => Ok(WindowsExecutable::Direct(path.to_string())),
        Ok(false) => {
            let (interpreter, interpreter_args) = parse_shebang(path)?;
            let (interpreter, interpreter_args) =
                normalize_env_shebang(interpreter, interpreter_args);
            Ok(WindowsExecutable::Interpreted {
                interpreter,
                interpreter_args,
                script: path.to_string(),
            })
        }
        Err(e) => Err(format!("Cannot read '{}': {}", path, e)),
    }
}

/// Map `(command, args)` to the form `CreateProcess` actually accepts.
/// Absolute / relative paths are inspected for PE magic vs. shebang;
/// bare names (`git`) pass through and let Windows resolve via `PATH`
/// + `PATHEXT`.
#[cfg(windows)]
pub fn resolve_for_windows(command: &str, args: &[&str]) -> Result<(String, Vec<String>), String> {
    match std::fs::metadata(command) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok((
            command.to_string(),
            args.iter().map(|s| s.to_string()).collect(),
        )),
        Err(e) => Err(format!("Cannot access '{}': {}", command, e)),
        Ok(_) => match resolve_windows_executable(command)? {
            WindowsExecutable::Direct(path) => {
                Ok((path, args.iter().map(|s| s.to_string()).collect()))
            }
            WindowsExecutable::Interpreted {
                interpreter,
                interpreter_args,
                script,
            } => {
                let mut new_args = Vec::with_capacity(interpreter_args.len() + 1 + args.len());
                new_args.extend(interpreter_args);
                new_args.push(script);
                new_args.extend(args.iter().map(|s| s.to_string()));
                Ok((interpreter, new_args))
            }
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_env_strips_env_indirection() {
        let (i, a) = normalize_env_shebang("/usr/bin/env".into(), vec!["python3".into()]);
        assert_eq!(i, "python3");
        assert!(a.is_empty());
    }

    #[test]
    fn normalize_env_passes_through_non_env() {
        let (i, a) = normalize_env_shebang("/usr/bin/python".into(), vec!["-u".into()]);
        assert_eq!(i, "/usr/bin/python");
        assert_eq!(a, vec!["-u".to_string()]);
    }

    #[test]
    fn normalize_env_skips_flags() {
        let (i, a) = normalize_env_shebang(
            "/usr/bin/env".into(),
            vec!["-S".into(), "python".into(), "-u".into()],
        );
        assert_eq!(i, "python");
        assert_eq!(a, vec!["-u".to_string()]);
    }

    #[test]
    fn normalize_env_skips_var_assignments() {
        let (i, a) = normalize_env_shebang(
            "/usr/bin/env".into(),
            vec!["FOO=bar".into(), "python".into()],
        );
        assert_eq!(i, "python");
        assert!(a.is_empty());
    }

    #[test]
    fn normalize_env_handles_env_exe() {
        let (i, a) = normalize_env_shebang("/usr/bin/env.exe".into(), vec!["python".into()]);
        assert_eq!(i, "python");
        assert!(a.is_empty());
    }
}
