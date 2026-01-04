use std::path::PathBuf;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum CommandError {
    #[error("File not found: {0}")]
    FileNotFound(PathBuf),

    #[error("Path is not a file: {0}")]
    NotAFile(PathBuf),

    #[error("Failed to read file: {0}")]
    FileReadError(#[from] std::io::Error),

    #[error("File size ({size} bytes) exceeds {max_size} bytes limit")]
    FileTooLarge { size: u64, max_size: u64 },

    #[error("Unsupported image format: {0:?}")]
    UnsupportedImageFormat(image::ImageFormat),

    #[error("Failed to decode image: {0}")]
    ImageDecodeError(#[from] image::ImageError),

    #[error("Invalid protocol: {0}")]
    InvalidProtocol(String),

    #[error("Encoding error: {0}")]
    EncodingError(String),
}

impl CommandError {
    /// Maps error to exit code following Unix convention
    /// - 0: Success
    /// - 1: General errors (validation, unsupported format, etc.)
    /// - 2: I/O errors (file not found, read errors)
    pub fn exit_code(&self) -> i32 {
        match self {
            CommandError::FileNotFound(_) => 2,
            CommandError::NotAFile(_) => 2,
            CommandError::FileReadError(_) => 2,
            CommandError::FileTooLarge { .. } => 1,
            CommandError::UnsupportedImageFormat(_) => 1,
            CommandError::ImageDecodeError(_) => 1,
            CommandError::InvalidProtocol(_) => 1,
            CommandError::EncodingError(_) => 1,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_file_not_found_exit_code() {
        let err = CommandError::FileNotFound(PathBuf::from("missing.txt"));
        assert_eq!(err.exit_code(), 2);
    }

    #[test]
    fn test_not_a_file_exit_code() {
        let err = CommandError::NotAFile(PathBuf::from("/tmp"));
        assert_eq!(err.exit_code(), 2);
    }

    #[test]
    fn test_file_too_large_exit_code() {
        let err = CommandError::FileTooLarge {
            size: 3_000_000,
            max_size: 2_000_000,
        };
        assert_eq!(err.exit_code(), 1);
    }

    #[test]
    fn test_unsupported_format_exit_code() {
        let err = CommandError::UnsupportedImageFormat(image::ImageFormat::Tiff);
        assert_eq!(err.exit_code(), 1);
    }

    #[test]
    fn test_invalid_protocol_exit_code() {
        let err = CommandError::InvalidProtocol("ascii".to_string());
        assert_eq!(err.exit_code(), 1);
    }

    #[test]
    fn test_error_display_messages() {
        let err = CommandError::FileNotFound(PathBuf::from("missing.txt"));
        assert!(err.to_string().contains("File not found"));
        assert!(err.to_string().contains("missing.txt"));

        let err = CommandError::FileTooLarge {
            size: 3_000_000,
            max_size: 2_000_000,
        };
        assert!(err.to_string().contains("exceeds"));
        assert!(err.to_string().contains("3000000"));
        assert!(err.to_string().contains("2000000"));
    }
}
