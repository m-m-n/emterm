//! File-path / size validation. Ported from `src-tauri/src/validation/file.rs`.

use crate::cli::error::CommandError;
use std::fs::File;
use std::path::{Path, PathBuf};

/// Validates file path exists and is a file (not directory)
pub fn validate_file_path(path: &Path) -> Result<PathBuf, CommandError> {
    if !path.exists() {
        return Err(CommandError::FileNotFound(path.to_owned()));
    }

    if !path.is_file() {
        return Err(CommandError::NotAFile(path.to_owned()));
    }

    Ok(path.to_owned())
}

/// Validates file size is within limit
pub fn validate_file_size(path: &Path, max_size: u64) -> Result<(), CommandError> {
    let metadata = std::fs::metadata(path)?;
    let size = metadata.len();

    if size > max_size {
        return Err(CommandError::FileTooLarge { size, max_size });
    }

    Ok(())
}

/// Opens file and validates both existence and size in one operation to avoid TOCTOU
pub fn open_and_validate_file(path: &Path, max_size: u64) -> Result<(File, PathBuf), CommandError> {
    let file = File::open(path).map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            CommandError::FileNotFound(path.to_owned())
        } else {
            CommandError::FileReadError(e)
        }
    })?;

    let metadata = file.metadata()?;

    if !metadata.is_file() {
        return Err(CommandError::NotAFile(path.to_owned()));
    }

    let size = metadata.len();
    if size > max_size {
        return Err(CommandError::FileTooLarge { size, max_size });
    }

    Ok((file, path.to_owned()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    #[test]
    fn test_validate_file_path_with_valid_file() {
        let mut temp_file = NamedTempFile::new().unwrap();
        writeln!(temp_file, "test content").unwrap();

        let result = validate_file_path(temp_file.path());
        assert!(result.is_ok());
    }

    #[test]
    fn test_validate_file_path_with_non_existent_file() {
        let result = validate_file_path(Path::new("/nonexistent/file.txt"));
        assert!(matches!(result, Err(CommandError::FileNotFound(_))));
    }

    #[test]
    fn test_validate_file_path_with_directory() {
        let result = validate_file_path(Path::new("/tmp"));
        assert!(matches!(result, Err(CommandError::NotAFile(_))));
    }

    #[test]
    fn test_validate_file_size_within_limit() {
        let mut temp_file = NamedTempFile::new().unwrap();
        writeln!(temp_file, "small content").unwrap();

        let result = validate_file_size(temp_file.path(), 1024);
        assert!(result.is_ok());
    }

    #[test]
    fn test_validate_file_size_at_limit() {
        let mut temp_file = NamedTempFile::new().unwrap();
        let content = "x".repeat(100);
        write!(temp_file, "{}", content).unwrap();
        temp_file.flush().unwrap();

        let metadata = std::fs::metadata(temp_file.path()).unwrap();
        let size = metadata.len();

        let result = validate_file_size(temp_file.path(), size);
        assert!(result.is_ok());
    }

    #[test]
    fn test_validate_file_size_exceeds_limit() {
        let mut temp_file = NamedTempFile::new().unwrap();
        let content = "x".repeat(1000);
        write!(temp_file, "{}", content).unwrap();
        temp_file.flush().unwrap();

        let result = validate_file_size(temp_file.path(), 100);
        assert!(matches!(result, Err(CommandError::FileTooLarge { .. })));
    }

    #[test]
    fn test_open_and_validate_file_success() {
        let mut temp_file = NamedTempFile::new().unwrap();
        writeln!(temp_file, "test content").unwrap();
        temp_file.flush().unwrap();

        let result = open_and_validate_file(temp_file.path(), 1024);
        assert!(result.is_ok());
        let (_file, _path) = result.unwrap();
    }

    #[test]
    fn test_open_and_validate_file_not_found() {
        let result = open_and_validate_file(Path::new("/nonexistent.txt"), 1024);
        assert!(matches!(result, Err(CommandError::FileNotFound(_))));
    }

    #[test]
    fn test_open_and_validate_file_too_large() {
        let mut temp_file = NamedTempFile::new().unwrap();
        let content = "x".repeat(1000);
        write!(temp_file, "{}", content).unwrap();
        temp_file.flush().unwrap();

        let result = open_and_validate_file(temp_file.path(), 100);
        assert!(matches!(result, Err(CommandError::FileTooLarge { .. })));
    }
}
