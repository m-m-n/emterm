//! `markdown` subcommand handler.
//!
//! Ported from `src-tauri/src/commands/markdown.rs`. The interactive
//! stdin loop (`run_interactive_loop`, navigate/image/quit commands) is
//! intentionally omitted — Phase A delivers only the non-interactive
//! emission path. The interactive loop is a separate SDD task.

use crate::cli::encoding::{base64, osc};
use crate::cli::error::CommandError;
use crate::cli::tmux;
use std::fs::File;
use std::io::{self, Read, Write};
use std::path::Path;
use uuid::Uuid;

/// Chunk size for base64 encoded Markdown (128KB)
const MARKDOWN_CHUNK_SIZE: usize = 128 * 1024;

/// Maximum markdown file size (10MB)
const MAX_MARKDOWN_SIZE: u64 = 10 * 1024 * 1024;

/// Compute the basedir (parent directory) from a canonical file path.
fn compute_basedir(file_path: &Path) -> Option<String> {
    file_path.parent().map(|p| p.to_string_lossy().into_owned())
}

/// Read a file and generate markdown OSC sequences with basedir.
///
/// `interactive` tags the end sequence with `interactive=1`. The native
/// port does not currently implement the interactive stdin loop, so the
/// caller passes `false` from [`execute_markdown_command`]; the
/// parameter is retained for forward compatibility (and for parity with
/// the src-tauri unit tests).
fn generate_markdown_output(file_path: &Path, interactive: bool) -> Result<String, CommandError> {
    let canonical = std::fs::canonicalize(file_path).map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            CommandError::FileNotFound(file_path.to_owned())
        } else {
            CommandError::FileReadError(e)
        }
    })?;

    let mut file = File::open(&canonical).map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            CommandError::FileNotFound(canonical.clone())
        } else {
            CommandError::FileReadError(e)
        }
    })?;

    let metadata = file.metadata()?;
    if !metadata.is_file() {
        return Err(CommandError::NotAFile(canonical));
    }

    if metadata.len() > MAX_MARKDOWN_SIZE {
        return Err(CommandError::FileTooLarge {
            size: metadata.len(),
            max_size: MAX_MARKDOWN_SIZE,
        });
    }

    let mut content = Vec::with_capacity(metadata.len() as usize);
    file.read_to_end(&mut content)?;

    let session_id = Uuid::new_v4();
    let encoded = base64::encode_base64(&content);
    drop(content);
    let chunks = base64::chunk_data(&encoded, MARKDOWN_CHUNK_SIZE);
    drop(encoded);

    let basedir = compute_basedir(&canonical);
    let sequence = osc::generate_markdown_osc(&session_id, chunks, basedir.as_deref(), interactive);

    Ok(sequence)
}

/// Executes the markdown command: reads file, encodes to base64, emits OSC sequences.
///
/// Unlike the src-tauri version, the native port does not enter an
/// interactive stdin loop; `interactive=1` is therefore never set.
pub fn execute_markdown_command(file_path: &Path) -> Result<(), CommandError> {
    let sequence = generate_markdown_output(file_path, false)?;
    output_to_stdout(&tmux::passthrough_if_needed(&sequence))?;
    Ok(())
}

/// Writes sequence to stdout with proper flushing
fn output_to_stdout(sequence: &str) -> io::Result<()> {
    let stdout = io::stdout();
    let mut handle = stdout.lock();
    handle.write_all(sequence.as_bytes())?;
    handle.flush()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    #[test]
    fn test_execute_markdown_command_with_valid_small_file() {
        let mut temp_file = NamedTempFile::new().unwrap();
        writeln!(temp_file, "# Hello World").unwrap();

        let result = execute_markdown_command(temp_file.path());
        assert!(result.is_ok());
    }

    #[test]
    fn test_execute_markdown_command_with_large_file() {
        let mut temp_file = NamedTempFile::new().unwrap();
        let large_content = "x".repeat(3 * 1024 * 1024);
        write!(temp_file, "{}", large_content).unwrap();
        temp_file.flush().unwrap();

        let result = execute_markdown_command(temp_file.path());
        assert!(result.is_ok());
    }

    #[test]
    fn test_generate_markdown_output_file_too_large() {
        let mut temp_file = NamedTempFile::new().unwrap();
        let large_content = "x".repeat(11 * 1024 * 1024);
        write!(temp_file, "{}", large_content).unwrap();
        temp_file.flush().unwrap();

        let result = generate_markdown_output(temp_file.path(), false);
        assert!(matches!(result, Err(CommandError::FileTooLarge { .. })));
    }

    #[test]
    fn test_execute_markdown_command_with_non_existent_file() {
        let result = execute_markdown_command(Path::new("/nonexistent/file.md"));
        assert!(matches!(result, Err(CommandError::FileNotFound(_))));
    }

    #[test]
    fn test_chunk_size_is_128kb() {
        assert_eq!(MARKDOWN_CHUNK_SIZE, 128 * 1024);
    }

    #[test]
    fn test_max_size_is_10mb() {
        assert_eq!(MAX_MARKDOWN_SIZE, 10 * 1024 * 1024);
    }

    #[test]
    fn test_output_to_stdout() {
        let test_sequence = "test output";
        let result = output_to_stdout(test_sequence);
        assert!(result.is_ok());
    }

    #[test]
    fn test_compute_basedir() {
        assert_eq!(
            compute_basedir(Path::new("/home/user/docs/file.md")),
            Some("/home/user/docs".to_string())
        );
    }

    #[test]
    fn test_compute_basedir_root() {
        assert_eq!(
            compute_basedir(Path::new("/file.md")),
            Some("/".to_string())
        );
    }

    #[test]
    fn test_generate_markdown_output_includes_basedir() {
        let mut temp_file = NamedTempFile::new().unwrap();
        writeln!(temp_file, "# Test").unwrap();

        let result = generate_markdown_output(temp_file.path(), false).unwrap();
        assert!(result.contains("basedir="));
    }

    #[test]
    fn test_generate_markdown_output_not_found() {
        let result = generate_markdown_output(Path::new("/nonexistent/file.md"), false);
        assert!(result.is_err());
    }

    #[test]
    fn test_generate_markdown_output_interactive_flag() {
        let mut temp_file = NamedTempFile::new().unwrap();
        writeln!(temp_file, "# Test").unwrap();

        let interactive = generate_markdown_output(temp_file.path(), true).unwrap();
        assert!(interactive.contains("interactive=1"));

        let non_interactive = generate_markdown_output(temp_file.path(), false).unwrap();
        assert!(!non_interactive.contains("interactive=1"));
    }

    #[test]
    fn test_execute_markdown_command_emits_begin_and_end() {
        let mut temp_file = NamedTempFile::new().unwrap();
        writeln!(temp_file, "# Test").unwrap();

        // Smoke: the function itself returns Ok. Detailed frame format
        // is covered by encoding::osc unit tests + integration tests.
        assert!(execute_markdown_command(temp_file.path()).is_ok());
    }

    #[test]
    fn test_generate_markdown_output_emits_chunk_frames() {
        let mut temp_file = NamedTempFile::new().unwrap();
        writeln!(temp_file, "# Hello").unwrap();

        let out = generate_markdown_output(temp_file.path(), false).unwrap();
        assert!(out.contains("\x1b]777;emterm;markdown;begin"));
        assert!(out.contains("\x1b]777;emterm;markdown;chunk"));
        assert!(out.contains("\x1b]777;emterm;markdown;end"));
    }

    #[test]
    fn test_generate_markdown_output_empty_file_skips_chunk() {
        let temp_file = NamedTempFile::new().unwrap();

        let out = generate_markdown_output(temp_file.path(), false).unwrap();
        assert!(out.contains("\x1b]777;emterm;markdown;begin"));
        assert!(out.contains("\x1b]777;emterm;markdown;end"));
        assert!(!out.contains("\x1b]777;emterm;markdown;chunk"));
    }
}
