//! `yaml` subcommand handler.
//! Ported from `src-tauri/src/commands/yaml.rs`.

use crate::cli::encoding::{base64, osc};
use crate::cli::error::CommandError;
use crate::cli::tmux;
use std::fs::File;
use std::io::{self, Read, Write};
use std::path::Path;
use uuid::Uuid;

/// Chunk size for base64 encoded YAML (128KB)
const YAML_CHUNK_SIZE: usize = 128 * 1024;

/// Executes the yaml command: reads file, encodes to base64, emits OSC sequences.
pub fn execute_yaml_command(file_path: &Path) -> Result<(), CommandError> {
    let mut file = File::open(file_path).map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            CommandError::FileNotFound(file_path.to_owned())
        } else {
            CommandError::FileReadError(e)
        }
    })?;

    let metadata = file.metadata()?;
    if !metadata.is_file() {
        return Err(CommandError::NotAFile(file_path.to_owned()));
    }

    let mut content = Vec::with_capacity(metadata.len() as usize);
    file.read_to_end(&mut content)?;

    let session_id = Uuid::new_v4();

    let encoded = base64::encode_base64(&content);
    drop(content);
    let chunks = base64::chunk_data(&encoded, YAML_CHUNK_SIZE);
    drop(encoded);

    let sequence = osc::generate_yaml_osc(&session_id, chunks);

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
    fn test_execute_yaml_command_with_valid_file() {
        let mut temp_file = NamedTempFile::new().unwrap();
        writeln!(temp_file, "key: value").unwrap();

        let result = execute_yaml_command(temp_file.path());
        assert!(result.is_ok());
    }

    #[test]
    fn test_execute_yaml_command_with_non_existent_file() {
        let result = execute_yaml_command(Path::new("/nonexistent/file.yaml"));
        assert!(matches!(result, Err(CommandError::FileNotFound(_))));
    }

    #[test]
    fn test_chunk_size_is_128kb() {
        assert_eq!(YAML_CHUNK_SIZE, 128 * 1024);
    }
}
