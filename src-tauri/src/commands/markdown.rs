use crate::encoding::{base64, osc};
use crate::error::CommandError;
use std::fs::File;
use std::io::{self, Read, Write};
use std::path::Path;
use uuid::Uuid;

/// Chunk size for base64 encoded Markdown (128KB)
const MARKDOWN_CHUNK_SIZE: usize = 128 * 1024;

/// Executes the markdown command: reads file, encodes to base64, generates OSC sequences
pub fn execute_markdown_command(file_path: &Path) -> Result<(), CommandError> {
    // Open file (validates existence and readability, no size limit)
    let mut file = File::open(file_path).map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            CommandError::FileNotFound(file_path.to_owned())
        } else {
            CommandError::FileReadError(e)
        }
    })?;

    // Check it's a file (not directory)
    let metadata = file.metadata()?;
    if !metadata.is_file() {
        return Err(CommandError::NotAFile(file_path.to_owned()));
    }

    // Read file content with pre-allocated buffer
    let mut content = Vec::with_capacity(metadata.len() as usize);
    file.read_to_end(&mut content)?;

    // Generate UUID for this session
    let session_id = Uuid::new_v4();

    // Encode to base64 and chunk, releasing intermediates early
    let encoded = base64::encode_base64(&content);
    drop(content);
    let chunks = base64::chunk_data(&encoded, MARKDOWN_CHUNK_SIZE);
    drop(encoded);

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
    fn test_execute_markdown_command_with_large_file() {
        let mut temp_file = NamedTempFile::new().unwrap();
        // Create a file larger than 2MB — should succeed (no size limit)
        let large_content = "x".repeat(3 * 1024 * 1024);
        write!(temp_file, "{}", large_content).unwrap();
        temp_file.flush().unwrap();

        let result = execute_markdown_command(temp_file.path());
        assert!(result.is_ok());
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
    fn test_output_to_stdout() {
        let test_sequence = "test output";
        let result = output_to_stdout(test_sequence);
        assert!(result.is_ok());
    }
}
