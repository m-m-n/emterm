use crate::encoding::{base64, osc};
use crate::error::CommandError;
use crate::validation::file;
use std::io::{self, Read, Write};
use std::path::Path;
use uuid::Uuid;

/// Maximum file size for Markdown files (2MB)
const MAX_MARKDOWN_SIZE: u64 = 2 * 1024 * 1024;

/// Chunk size for base64 encoded Markdown (64KB)
const MARKDOWN_CHUNK_SIZE: usize = 64 * 1024;

/// Executes the markdown command: reads file, encodes to base64, generates OSC sequences
pub fn execute_markdown_command(file_path: &Path) -> Result<(), CommandError> {
    // Open and validate file in one operation (prevents TOCTOU)
    let (mut file, _validated_path) = file::open_and_validate_file(file_path, MAX_MARKDOWN_SIZE)?;

    // Read file content from the open handle
    let mut content = Vec::new();
    file.read_to_end(&mut content)?;

    // Generate UUID for this session
    let session_id = Uuid::new_v4();

    // Encode to base64 and chunk
    let encoded = base64::encode_base64(&content);
    let chunks = base64::chunk_data(&encoded, MARKDOWN_CHUNK_SIZE);

    // Generate OSC sequences
    let sequence = osc::generate_markdown_osc(&session_id, chunks);

    // Output to stdout (wrap in DCS passthrough when inside tmux)
    output_to_stdout(&super::tmux::passthrough_if_needed(&sequence))?;

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
    fn test_execute_markdown_command_with_oversized_file() {
        let mut temp_file = NamedTempFile::new().unwrap();
        // Create a file larger than 2MB
        let large_content = "x".repeat(3 * 1024 * 1024);
        write!(temp_file, "{}", large_content).unwrap();
        temp_file.flush().unwrap();

        let result = execute_markdown_command(temp_file.path());
        assert!(matches!(result, Err(CommandError::FileTooLarge { .. })));
    }

    #[test]
    fn test_execute_markdown_command_with_non_existent_file() {
        let result = execute_markdown_command(Path::new("/nonexistent/file.md"));
        assert!(matches!(result, Err(CommandError::FileNotFound(_))));
    }

    #[test]
    fn test_output_to_stdout() {
        let test_sequence = "test output";
        let result = output_to_stdout(test_sequence);
        assert!(result.is_ok());
    }
}
