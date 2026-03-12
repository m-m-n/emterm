use crate::encoding::{base64, osc};
use crate::error::CommandError;
use std::fs::File;
use std::io::{self, Read, Write};
use std::path::Path;
use uuid::Uuid;

/// Chunk size for base64 encoded download data (128KB)
const DOWNLOAD_CHUNK_SIZE: usize = 128 * 1024;

/// Sanitize a filename by stripping path components and traversal sequences.
///
/// Returns the basename only, with no path separators or `..` components.
/// Also strips semicolons (OSC field delimiter) and control characters.
pub fn sanitize_filename(name: &str) -> String {
    // Take the last component after any path separator
    let basename = name
        .rsplit(|c| c == '/' || c == '\\')
        .next()
        .unwrap_or(name);

    // Reject pure traversal/dot names
    if basename.is_empty() || basename == "." || basename == ".." {
        return "download".to_string();
    }

    // Remove characters unsafe for OSC embedding: semicolons and control chars
    let sanitized: String = basename
        .chars()
        .filter(|c| *c != ';' && !c.is_control())
        .collect();

    if sanitized.is_empty() {
        "download".to_string()
    } else {
        sanitized
    }
}

/// Executes the download command with a file path argument.
pub fn execute_download_command(file_path: &Path) -> Result<(), CommandError> {
    let mut file = File::open(file_path).map_err(|e| match e.kind() {
        std::io::ErrorKind::NotFound => CommandError::FileNotFound(file_path.to_owned()),
        std::io::ErrorKind::PermissionDenied => {
            CommandError::PermissionDenied(file_path.to_owned())
        }
        _ => CommandError::FileReadError(e),
    })?;

    let metadata = file.metadata()?;
    if !metadata.is_file() {
        return Err(CommandError::NotAFile(file_path.to_owned()));
    }

    let file_size = metadata.len();
    let filename = sanitize_filename(
        file_path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("download"),
    );

    let mut content = Vec::with_capacity(file_size as usize);
    file.read_to_end(&mut content)?;

    output_download_sequence(&filename, file_size, &content)
}

/// Executes the download command reading from stdin.
pub fn execute_download_from_stdin(name: &str) -> Result<(), CommandError> {
    let filename = sanitize_filename(name);

    let mut content = Vec::new();
    io::stdin().read_to_end(&mut content)?;

    let file_size = content.len() as u64;
    output_download_sequence(&filename, file_size, &content)
}

/// Generate and output download OSC sequences.
fn output_download_sequence(
    filename: &str,
    file_size: u64,
    content: &[u8],
) -> Result<(), CommandError> {
    let session_id = Uuid::new_v4();

    let encoded = base64::encode_base64(content);
    let chunks = base64::chunk_data(&encoded, DOWNLOAD_CHUNK_SIZE);

    let sequence = osc::generate_download_osc(&session_id, filename, file_size, chunks);

    let stdout = io::stdout();
    let mut handle = stdout.lock();
    handle
        .write_all(super::tmux::passthrough_if_needed(&sequence).as_bytes())
        .map_err(CommandError::FileReadError)?;
    handle.flush().map_err(CommandError::FileReadError)?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    // --- sanitize_filename tests ---

    #[test]
    fn test_sanitize_simple_filename() {
        assert_eq!(sanitize_filename("test.txt"), "test.txt");
    }

    #[test]
    fn test_sanitize_strips_unix_path() {
        assert_eq!(sanitize_filename("/home/user/file.txt"), "file.txt");
    }

    #[test]
    fn test_sanitize_strips_windows_path() {
        assert_eq!(sanitize_filename("C:\\Users\\file.txt"), "file.txt");
    }

    #[test]
    fn test_sanitize_strips_traversal() {
        assert_eq!(sanitize_filename("../../etc/passwd"), "passwd");
    }

    #[test]
    fn test_sanitize_double_dot_in_name() {
        assert_eq!(sanitize_filename(".."), "download");
    }

    #[test]
    fn test_sanitize_empty_string() {
        assert_eq!(sanitize_filename(""), "download");
    }

    #[test]
    fn test_sanitize_preserves_dots_in_filename() {
        assert_eq!(sanitize_filename("my.file.tar.gz"), "my.file.tar.gz");
    }

    #[test]
    fn test_sanitize_preserves_double_dot_in_name() {
        // Legitimate filenames with ".." should be preserved
        assert_eq!(sanitize_filename("file..v2.txt"), "file..v2.txt");
    }

    #[test]
    fn test_sanitize_strips_semicolons() {
        assert_eq!(sanitize_filename("evil;inject=val.txt"), "evilinject=val.txt");
    }

    #[test]
    fn test_sanitize_mixed_separators() {
        assert_eq!(sanitize_filename("/foo\\bar/baz.txt"), "baz.txt");
    }

    // --- execute_download_command tests ---

    #[test]
    fn test_execute_download_command_with_valid_file() {
        let mut temp_file = NamedTempFile::new().unwrap();
        writeln!(temp_file, "Hello World").unwrap();

        let result = execute_download_command(temp_file.path());
        assert!(result.is_ok());
    }

    #[test]
    fn test_execute_download_command_with_non_existent_file() {
        let result = execute_download_command(Path::new("/nonexistent/file.txt"));
        assert!(matches!(result, Err(CommandError::FileNotFound(_))));
    }

    #[test]
    fn test_execute_download_command_with_directory() {
        let result = execute_download_command(Path::new("/tmp"));
        assert!(matches!(result, Err(CommandError::NotAFile(_))));
    }

    #[test]
    fn test_execute_download_command_with_empty_file() {
        let temp_file = NamedTempFile::new().unwrap();
        let result = execute_download_command(temp_file.path());
        assert!(result.is_ok());
    }

    #[test]
    fn test_chunk_size_is_128kb() {
        assert_eq!(DOWNLOAD_CHUNK_SIZE, 128 * 1024);
    }
}
