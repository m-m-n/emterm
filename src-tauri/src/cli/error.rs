//! Typed error surface for all CLI subcommands.
//!
//! Mirrors `src-tauri/src/error.rs` variant-for-variant. The `Display`
//! impl resolves the locale via [`crate::cli::active_locale`] and
//! formats via the message helpers in [`crate::cli::messages`]. There is
//! no `rust-i18n` dependency.

use crate::cli::messages;
use std::fmt;
use std::path::PathBuf;

#[derive(Debug)]
pub enum CommandError {
    FileNotFound(PathBuf),

    NotAFile(PathBuf),

    FileReadError(std::io::Error),

    FileTooLarge {
        size: u64,
        max_size: u64,
    },

    UnsupportedImageFormat(image::ImageFormat),

    ImageDecodeError(image::ImageError),

    InvalidProtocol(String),

    EncodingError(String),

    NameRequired,

    PermissionDenied(PathBuf),

    UnsupportedExtension {
        path: PathBuf,
        allowed: &'static [&'static str],
    },
}

impl fmt::Display for CommandError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let loc = crate::cli::active_locale();
        let s = match self {
            CommandError::FileNotFound(path) => messages::err_file_not_found(loc, path),
            CommandError::NotAFile(path) => messages::err_not_a_file(loc, path),
            CommandError::FileReadError(source) => messages::err_file_read_error(loc, source),
            CommandError::FileTooLarge { size, max_size } => {
                messages::err_file_too_large(loc, *size, *max_size)
            }
            CommandError::UnsupportedImageFormat(format) => {
                messages::err_unsupported_image_format(loc, *format)
            }
            CommandError::ImageDecodeError(source) => messages::err_image_decode_error(loc, source),
            CommandError::InvalidProtocol(protocol) => {
                messages::err_invalid_protocol(loc, protocol)
            }
            CommandError::EncodingError(msg) => messages::err_encoding_error(loc, msg),
            CommandError::NameRequired => messages::err_name_required(loc),
            CommandError::PermissionDenied(path) => messages::err_permission_denied(loc, path),
            CommandError::UnsupportedExtension { path, allowed } => {
                messages::err_unsupported_extension(loc, path, allowed)
            }
        };
        f.write_str(&s)
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
    /// - 2: I/O errors (file not found, read errors, permission denied, usage errors)
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
            CommandError::NameRequired => 2,
            CommandError::PermissionDenied(_) => 2,
            CommandError::UnsupportedExtension { .. } => 1,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::set_active_locale_for_test;
    use crate::i18n::Locale;

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
    fn test_name_required_exit_code() {
        let err = CommandError::NameRequired;
        assert_eq!(err.exit_code(), 2);
    }

    #[test]
    fn test_permission_denied_exit_code() {
        let err = CommandError::PermissionDenied(PathBuf::from("/etc/shadow"));
        assert_eq!(err.exit_code(), 2);
    }

    #[test]
    fn test_encoding_error_exit_code() {
        let err = CommandError::EncodingError("dimensions exceed".to_string());
        assert_eq!(err.exit_code(), 1);
    }

    #[test]
    fn test_unsupported_extension_exit_code() {
        let err = CommandError::UnsupportedExtension {
            path: PathBuf::from("file.txt"),
            allowed: &["html", "htm"],
        };
        assert_eq!(err.exit_code(), 1);
    }

    #[test]
    fn test_unsupported_extension_display_includes_path_and_allowed() {
        set_active_locale_for_test(Locale::En);
        let err = CommandError::UnsupportedExtension {
            path: PathBuf::from("file.txt"),
            allowed: &["html", "htm"],
        };
        let msg = err.to_string();
        assert!(msg.contains("file.txt"));
        assert!(msg.contains("html"));
        assert!(msg.contains("htm"));
    }

    // Single test exercises both locales sequentially; the test helper
    // overrides the cached locale serially under a mutex so this is safe
    // with parallel test execution as long as a given test does not
    // observe the cached locale while a *different* test is mid-swap.
    // We mark this test as the canonical locale test and gate other
    // locale-sensitive assertions on the messages helper (which takes
    // the locale by argument) to avoid races.
    #[test]
    fn test_error_display_messages_localized() {
        set_active_locale_for_test(Locale::En);
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

        set_active_locale_for_test(Locale::Ja);
        let err = CommandError::FileNotFound(PathBuf::from("test.txt"));
        let msg = err.to_string();
        assert!(msg.contains("test.txt"));
        assert!(msg.contains("ファイルが見つかりません"));

        // Reset to English for any subsequent tests that observe the
        // cache directly.
        set_active_locale_for_test(Locale::En);
    }
}
