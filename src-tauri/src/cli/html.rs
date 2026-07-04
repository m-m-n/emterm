//! `html` subcommand handler.
//!
//! Mirrors the responsibilities of the markdown handler (canonicalize /
//! validate / encode / emit) with an added file-extension check per
//! feature-docs/html-viewer/tasks/task0001.md. Unlike markdown, the
//! emitted OSC frame carries no `format`/`render`/`interactive` params —
//! the document renders with its own styles only, and this subcommand
//! has no interactive stdin loop.

use crate::cli::encoding::{base64, osc};
use crate::cli::error::CommandError;
use crate::cli::tmux;
use crate::cli::validation::file;
use std::io::{self, Read, Write};
use std::path::Path;
use uuid::Uuid;

/// Chunk size for base64 encoded HTML (128KB), matching the markdown subcommand.
const HTML_CHUNK_SIZE: usize = 128 * 1024;

/// Maximum HTML file size (10MB), matching the markdown subcommand.
const MAX_HTML_SIZE: u64 = 10 * 1024 * 1024;

/// Extensions accepted by the `html` subcommand (case-insensitive).
const ALLOWED_EXTENSIONS: &[&str] = &["html", "htm"];

/// Compute the basedir (parent directory) from a canonical file path.
fn compute_basedir(file_path: &Path) -> Option<String> {
    file_path.parent().map(|p| p.to_string_lossy().into_owned())
}

/// Read a file and generate `html` OSC sequences with basedir.
///
/// Validation order: existence / regular-file / size (`open_and_validate_file`,
/// shared with the image subcommand) before the extension check, so a
/// missing path or a directory always surfaces `FileNotFound` /
/// `NotAFile` regardless of its (lack of) extension.
fn generate_html_output(file_path: &Path) -> Result<String, CommandError> {
    let (mut fh, validated_path) = file::open_and_validate_file(file_path, MAX_HTML_SIZE)?;

    file::validate_extension(&validated_path, ALLOWED_EXTENSIONS)?;

    let canonical = std::fs::canonicalize(&validated_path).map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            CommandError::FileNotFound(validated_path.clone())
        } else {
            CommandError::FileReadError(e)
        }
    })?;

    let mut content = Vec::new();
    fh.read_to_end(&mut content)?;

    let session_id = Uuid::new_v4();
    let encoded = base64::encode_base64(&content);
    drop(content);
    let chunks = base64::chunk_data(&encoded, HTML_CHUNK_SIZE);
    drop(encoded);

    let basedir = compute_basedir(&canonical);
    let sequence = osc::generate_html_osc(&session_id, chunks, basedir.as_deref());

    Ok(sequence)
}

/// Executes the html command: validates, reads file, encodes to base64,
/// emits OSC sequences.
pub fn execute_html_command(file_path: &Path) -> Result<(), CommandError> {
    let sequence = generate_html_output(file_path)?;
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
    fn test_execute_html_command_with_valid_small_file() {
        let mut temp_file = NamedTempFile::with_suffix(".html").unwrap();
        writeln!(temp_file, "<html><body>Hello</body></html>").unwrap();
        temp_file.flush().unwrap();

        let result = execute_html_command(temp_file.path());
        assert!(result.is_ok());
    }

    #[test]
    fn test_generate_html_output_includes_basedir() {
        let mut temp_file = NamedTempFile::with_suffix(".html").unwrap();
        writeln!(temp_file, "<html></html>").unwrap();
        temp_file.flush().unwrap();

        let result = generate_html_output(temp_file.path()).unwrap();
        assert!(result.contains("basedir="));
    }

    #[test]
    fn test_generate_html_output_emits_begin_chunk_end() {
        let mut temp_file = NamedTempFile::with_suffix(".html").unwrap();
        writeln!(temp_file, "<html></html>").unwrap();
        temp_file.flush().unwrap();

        let out = generate_html_output(temp_file.path()).unwrap();
        assert!(out.contains("\x1b]777;emterm;html;begin"));
        assert!(out.contains("\x1b]777;emterm;html;chunk"));
        assert!(out.contains("\x1b]777;emterm;html;end"));
    }

    // --- AC-2: extension validation wired into the handler ---

    #[test]
    fn test_execute_html_command_rejects_non_html_extension() {
        let mut temp_file = NamedTempFile::with_suffix(".txt").unwrap();
        writeln!(temp_file, "not html").unwrap();
        temp_file.flush().unwrap();

        let result = execute_html_command(temp_file.path());
        assert!(matches!(
            result,
            Err(CommandError::UnsupportedExtension { .. })
        ));
    }

    #[test]
    fn test_execute_html_command_accepts_htm_extension() {
        let mut temp_file = NamedTempFile::with_suffix(".htm").unwrap();
        writeln!(temp_file, "<html></html>").unwrap();
        temp_file.flush().unwrap();

        let result = execute_html_command(temp_file.path());
        assert!(result.is_ok());
    }

    // --- AC-3: exactly-10MB boundary ---

    #[test]
    fn test_execute_html_command_with_file_exactly_at_max_size() {
        let mut temp_file = NamedTempFile::with_suffix(".html").unwrap();
        let content = vec![b'x'; MAX_HTML_SIZE as usize];
        temp_file.write_all(&content).unwrap();
        temp_file.flush().unwrap();

        let result = execute_html_command(temp_file.path());
        assert!(result.is_ok());
    }

    #[test]
    fn test_execute_html_command_with_file_one_byte_over_max_size() {
        let mut temp_file = NamedTempFile::with_suffix(".html").unwrap();
        let content = vec![b'x'; MAX_HTML_SIZE as usize + 1];
        temp_file.write_all(&content).unwrap();
        temp_file.flush().unwrap();

        let result = execute_html_command(temp_file.path());
        assert!(matches!(result, Err(CommandError::FileTooLarge { .. })));
    }

    // --- AC-4: missing path / directory path ---

    #[test]
    fn test_execute_html_command_with_non_existent_file() {
        let result = execute_html_command(Path::new("/nonexistent/file.html"));
        assert!(matches!(result, Err(CommandError::FileNotFound(_))));
    }

    #[test]
    fn test_execute_html_command_with_directory_path() {
        let result = execute_html_command(Path::new("/tmp"));
        assert!(matches!(result, Err(CommandError::NotAFile(_))));
    }

    #[test]
    fn test_chunk_size_is_128kb() {
        assert_eq!(HTML_CHUNK_SIZE, 128 * 1024);
    }

    #[test]
    fn test_max_size_is_10mb() {
        assert_eq!(MAX_HTML_SIZE, 10 * 1024 * 1024);
    }

    #[test]
    fn test_compute_basedir() {
        assert_eq!(
            compute_basedir(Path::new("/home/user/docs/file.html")),
            Some("/home/user/docs".to_string())
        );
    }
}
