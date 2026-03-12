use crate::encoding::{base64, osc};
use crate::error::CommandError;
use std::fs::File;
use std::io::{self, Read, Write};
use std::path::Path;
use uuid::Uuid;

/// Chunk size for raw file reads (8 MiB).
/// 8 MiB raw produces ~10.7 MiB base64, well within the WASM parser's
/// MAX_OSC_LEN = 16 MiB limit.
const DOWNLOAD_CHUNK_SIZE: usize = 8 * 1024 * 1024;

/// Sanitize a filename by stripping path components and traversal sequences.
///
/// Returns the basename only, with no path separators or `..` components.
/// Also strips semicolons (OSC field delimiter) and control characters.
pub fn sanitize_filename(name: &str) -> String {
    // Take the last component after any path separator
    let basename = name
        .rsplit(['/', '\\'])
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
/// Reads in fixed-size chunks for constant memory usage.
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

    let session_id = Uuid::new_v4();
    let stdout = io::stdout();
    let mut handle = stdout.lock();

    // Output begin sequence
    let begin = osc::generate_download_osc_begin(&session_id, &filename, file_size);
    write_osc_sequence(&mut handle, &begin)?;

    // Read and output chunks
    let mut buffer = vec![0u8; DOWNLOAD_CHUNK_SIZE];
    let mut seq = 0;

    loop {
        let bytes_read = read_full_chunk(&mut file, &mut buffer)?;
        if bytes_read == 0 {
            break;
        }

        let encoded = base64::encode_base64(&buffer[..bytes_read]);
        let chunk = osc::generate_download_osc_chunk(&session_id, seq, &encoded);
        write_osc_sequence(&mut handle, &chunk)?;
        seq += 1;
    }

    // Output end sequence
    let end = osc::generate_download_osc_end(&session_id);
    write_osc_sequence(&mut handle, &end)?;

    Ok(())
}

/// Executes the download command reading from stdin.
/// stdin is fully buffered because size is unknown upfront.
pub fn execute_download_from_stdin(name: &str) -> Result<(), CommandError> {
    let filename = sanitize_filename(name);

    let mut content = Vec::new();
    io::stdin().read_to_end(&mut content)?;

    let file_size = content.len() as u64;
    let session_id = Uuid::new_v4();
    let stdout = io::stdout();
    let mut handle = stdout.lock();

    // Output begin sequence
    let begin = osc::generate_download_osc_begin(&session_id, &filename, file_size);
    write_osc_sequence(&mut handle, &begin)?;

    // Chunk base64 at the same size as the file path code path
    let encoded = base64::encode_base64(&content);
    let base64_chunk_size = DOWNLOAD_CHUNK_SIZE * 4 / 3;
    let chunks = base64::chunk_data(&encoded, base64_chunk_size);
    for (seq, data) in chunks.iter().enumerate() {
        let chunk = osc::generate_download_osc_chunk(&session_id, seq, data);
        write_osc_sequence(&mut handle, &chunk)?;
    }

    // Output end sequence
    let end = osc::generate_download_osc_end(&session_id);
    write_osc_sequence(&mut handle, &end)?;

    Ok(())
}

/// Write a single OSC sequence to stdout, applying tmux passthrough if needed.
fn write_osc_sequence(handle: &mut io::StdoutLock, sequence: &str) -> Result<(), CommandError> {
    handle
        .write_all(super::tmux::passthrough_if_needed(sequence).as_bytes())
        .map_err(CommandError::FileReadError)?;
    handle.flush().map_err(CommandError::FileReadError)?;
    Ok(())
}

/// Read a full chunk from the reader, handling partial reads.
fn read_full_chunk<R: Read>(reader: &mut R, buffer: &mut [u8]) -> Result<usize, io::Error> {
    let mut total = 0;
    while total < buffer.len() {
        match reader.read(&mut buffer[total..]) {
            Ok(0) => break,
            Ok(n) => total += n,
            Err(ref e) if e.kind() == io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(e),
        }
    }
    Ok(total)
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
        assert_eq!(sanitize_filename("file..v2.txt"), "file..v2.txt");
    }

    #[test]
    fn test_sanitize_strips_semicolons() {
        assert_eq!(
            sanitize_filename("evil;inject=val.txt"),
            "evilinject=val.txt"
        );
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
    fn test_chunk_size_is_8mib() {
        assert_eq!(DOWNLOAD_CHUNK_SIZE, 8 * 1024 * 1024);
    }

    // --- read_full_chunk tests ---

    #[test]
    fn test_read_full_chunk_complete() {
        let data = vec![1u8; 100];
        let mut cursor = io::Cursor::new(data.clone());
        let mut buffer = vec![0u8; 100];
        let n = read_full_chunk(&mut cursor, &mut buffer).unwrap();
        assert_eq!(n, 100);
        assert_eq!(&buffer[..n], &data[..]);
    }

    #[test]
    fn test_read_full_chunk_partial_eof() {
        let data = vec![42u8; 50];
        let mut cursor = io::Cursor::new(data.clone());
        let mut buffer = vec![0u8; 100];
        let n = read_full_chunk(&mut cursor, &mut buffer).unwrap();
        assert_eq!(n, 50);
        assert_eq!(&buffer[..n], &data[..]);
    }

    #[test]
    fn test_read_full_chunk_empty() {
        let data: Vec<u8> = vec![];
        let mut cursor = io::Cursor::new(data);
        let mut buffer = vec![0u8; 100];
        let n = read_full_chunk(&mut cursor, &mut buffer).unwrap();
        assert_eq!(n, 0);
    }
}
