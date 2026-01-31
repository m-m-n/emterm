use rust_i18n::t;
use std::fmt;
use std::path::PathBuf;

#[derive(Debug)]
pub enum CommandError {
    FileNotFound(PathBuf),

    NotAFile(PathBuf),

    FileReadError(std::io::Error),

    FileTooLarge { size: u64, max_size: u64 },

    UnsupportedImageFormat(image::ImageFormat),

    ImageDecodeError(image::ImageError),

    InvalidProtocol(String),

    EncodingError(String),
}

impl fmt::Display for CommandError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CommandError::FileNotFound(path) => {
                write!(f, "{}", t!("error.fileNotFound", path = path.display()))
            }
            CommandError::NotAFile(path) => {
                write!(f, "{}", t!("error.notAFile", path = path.display()))
            }
            CommandError::FileReadError(source) => {
                write!(f, "{}", t!("error.fileReadError", error = source))
            }
            CommandError::FileTooLarge { size, max_size } => {
                write!(
                    f,
                    "{}",
                    t!("error.fileTooLarge", size = size, maxSize = max_size)
                )
            }
            CommandError::UnsupportedImageFormat(format) => {
                write!(
                    f,
                    "{}",
                    t!(
                        "error.unsupportedImageFormat",
                        format = format!("{:?}", format)
                    )
                )
            }
            CommandError::ImageDecodeError(source) => {
                write!(f, "{}", t!("error.imageDecodeError", error = source))
            }
            CommandError::InvalidProtocol(protocol) => {
                write!(f, "{}", t!("error.invalidProtocol", protocol = protocol))
            }
            CommandError::EncodingError(msg) => {
                write!(f, "{}", t!("error.encodingError", error = msg))
            }
        }
    }
}

impl std::error::Error for CommandError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            CommandError::FileReadError(source) => Some(source),
            CommandError::ImageDecodeError(source) => Some(source),
            _ => None,
        }
    }
}

impl From<std::io::Error> for CommandError {
    fn from(err: std::io::Error) -> Self {
        CommandError::FileReadError(err)
    }
}

impl From<image::ImageError> for CommandError {
    fn from(err: image::ImageError) -> Self {
        CommandError::ImageDecodeError(err)
    }
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

    // Single test for both locales to avoid race conditions with global locale state
    #[test]
    fn test_error_display_messages_localized() {
        // English
        rust_i18n::set_locale("en");

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

        // Japanese
        rust_i18n::set_locale("ja");

        let err = CommandError::FileNotFound(PathBuf::from("test.txt"));
        let msg = err.to_string();
        assert!(msg.contains("test.txt"));
        assert!(
            msg.contains("\u{30d5}\u{30a1}\u{30a4}\u{30eb}\u{304c}\u{898b}\u{3064}\u{304b}\u{308a}\u{307e}\u{305b}\u{3093}")
        );

        // Reset to English
        rust_i18n::set_locale("en");
    }
}
