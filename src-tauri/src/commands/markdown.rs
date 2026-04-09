use crate::encoding::{base64, osc};
use crate::error::CommandError;
use std::fs::File;
use std::io::{self, BufRead, IsTerminal, Read, Write};
use std::path::{Path, PathBuf};
use uuid::Uuid;

/// Chunk size for base64 encoded Markdown (128KB)
const MARKDOWN_CHUNK_SIZE: usize = 128 * 1024;

/// Maximum markdown file size (10MB)
const MAX_MARKDOWN_SIZE: u64 = 10 * 1024 * 1024;

/// Maximum image file size (50MB)
const MAX_IMAGE_SIZE: u64 = 50 * 1024 * 1024;

/// Known image file extensions and their MIME types
const IMAGE_EXTENSIONS: &[(&str, &str)] = &[
    ("png", "image/png"),
    ("jpg", "image/jpeg"),
    ("jpeg", "image/jpeg"),
    ("gif", "image/gif"),
    ("webp", "image/webp"),
    ("svg", "image/svg+xml"),
    ("bmp", "image/bmp"),
    ("ico", "image/x-icon"),
];

/// Interactive command parsed from stdin
#[derive(Debug, PartialEq)]
enum InteractiveCommand {
    Navigate(PathBuf),
    Image { request_id: String, path: PathBuf },
    Quit,
}

/// Parse a line from stdin into an interactive command.
/// Returns None for unrecognized or empty lines.
fn parse_command(line: &str) -> Option<InteractiveCommand> {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return None;
    }

    if trimmed == "quit" {
        return Some(InteractiveCommand::Quit);
    }

    if let Some(path_str) = trimmed.strip_prefix("navigate ") {
        let path_str = path_str.trim();
        if !path_str.is_empty() {
            return Some(InteractiveCommand::Navigate(PathBuf::from(path_str)));
        }
    }

    if let Some(rest) = trimmed.strip_prefix("image ") {
        let rest = rest.trim();
        // Split on first space: "REQ_ID PATH"
        if let Some(space_pos) = rest.find(' ') {
            let request_id = rest[..space_pos].to_string();
            let path_str = rest[space_pos + 1..].trim();
            if !request_id.is_empty() && !path_str.is_empty() {
                return Some(InteractiveCommand::Image {
                    request_id,
                    path: PathBuf::from(path_str),
                });
            }
        }
    }

    None
}

/// Detect MIME type from file extension.
/// Returns None if the extension is not a recognized image type.
fn detect_mime_type(path: &Path) -> Option<&'static str> {
    let ext = path.extension()?.to_str()?.to_lowercase();
    IMAGE_EXTENSIONS
        .iter()
        .find(|(e, _)| *e == ext.as_str())
        .map(|(_, mime)| *mime)
}

/// Validate that a path has a .md extension (case-insensitive).
fn is_markdown_file(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| ext.eq_ignore_ascii_case("md"))
}

/// Compute the basedir (parent directory) from a canonical file path.
fn compute_basedir(file_path: &Path) -> Option<String> {
    file_path.parent().map(|p| p.to_string_lossy().into_owned())
}

/// Read a file and generate markdown OSC sequences with basedir.
fn generate_markdown_output(file_path: &Path) -> Result<String, CommandError> {
    let canonical = std::fs::canonicalize(file_path).map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            CommandError::FileNotFound(file_path.to_owned())
        } else {
            CommandError::FileReadError(e)
        }
    })?;

    let mut file = File::open(&canonical).map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            CommandError::FileNotFound(canonical.clone())
        } else {
            CommandError::FileReadError(e)
        }
    })?;

    let metadata = file.metadata()?;
    if !metadata.is_file() {
        return Err(CommandError::NotAFile(canonical));
    }

    if metadata.len() > MAX_MARKDOWN_SIZE {
        return Err(CommandError::FileTooLarge {
            size: metadata.len(),
            max_size: MAX_MARKDOWN_SIZE,
        });
    }

    let mut content = Vec::with_capacity(metadata.len() as usize);
    file.read_to_end(&mut content)?;

    let session_id = Uuid::new_v4();
    let encoded = base64::encode_base64(&content);
    drop(content);
    let chunks = base64::chunk_data(&encoded, MARKDOWN_CHUNK_SIZE);
    drop(encoded);

    let basedir = compute_basedir(&canonical);
    let sequence = osc::generate_markdown_osc(&session_id, chunks, basedir.as_deref());

    Ok(sequence)
}

/// Generate image response OSC for a given image file path.
fn generate_image_response(request_id: &str, file_path: &Path) -> String {
    // Canonicalize path
    let canonical = match std::fs::canonicalize(file_path) {
        Ok(p) => p,
        Err(e) => {
            return osc::generate_image_error_osc(request_id, &e.to_string());
        }
    };

    // Validate image extension
    let mime_type = match detect_mime_type(&canonical) {
        Some(mime) => mime,
        None => {
            return osc::generate_image_error_osc(request_id, "Unsupported image format");
        }
    };

    // Check file size
    match std::fs::metadata(&canonical) {
        Ok(meta) => {
            if meta.len() > MAX_IMAGE_SIZE {
                return osc::generate_image_error_osc(
                    request_id,
                    &format!(
                        "File too large: {} bytes (limit: {} bytes)",
                        meta.len(),
                        MAX_IMAGE_SIZE
                    ),
                );
            }
        }
        Err(e) => {
            return osc::generate_image_error_osc(request_id, &e.to_string());
        }
    }

    // Read file
    let data = match std::fs::read(&canonical) {
        Ok(d) => d,
        Err(e) => {
            return osc::generate_image_error_osc(request_id, &e.to_string());
        }
    };

    // Base64 encode
    let encoded = base64::encode_base64(&data);

    osc::generate_image_response_osc(request_id, mime_type, &encoded)
}

/// Executes the markdown command: reads file, encodes to base64, generates OSC sequences.
/// When stdin is a TTY, enters an interactive loop for navigate/image/quit commands.
pub fn execute_markdown_command(file_path: &Path) -> Result<(), CommandError> {
    let sequence = generate_markdown_output(file_path)?;

    // Output to stdout (wrap in DCS passthrough when inside tmux)
    output_to_stdout(&super::tmux::passthrough_if_needed(&sequence))?;

    // If stdin is a TTY, enter interactive mode
    if io::stdin().is_terminal() {
        run_interactive_loop();
    }

    Ok(())
}

/// Run the interactive stdin loop, reading commands line by line.
fn run_interactive_loop() {
    let stdin = io::stdin();
    let reader = io::BufReader::new(stdin.lock());

    for line in reader.lines() {
        let line = match line {
            Ok(l) => l,
            Err(_) => break, // EOF or read error
        };

        let cmd = match parse_command(&line) {
            Some(c) => c,
            None => {
                if !line.trim().is_empty() {
                    eprintln!("[WARN][BACKEND] Unknown command: {}", line.trim());
                }
                continue;
            }
        };

        match cmd {
            InteractiveCommand::Quit => break,

            InteractiveCommand::Navigate(path) => {
                // Validate .md extension
                if !is_markdown_file(&path) {
                    eprintln!(
                        "[WARN][BACKEND] Navigate rejected: not a .md file: {}",
                        path.display()
                    );
                    continue;
                }

                match generate_markdown_output(&path) {
                    Ok(sequence) => {
                        if let Err(e) =
                            output_to_stdout(&super::tmux::passthrough_if_needed(&sequence))
                        {
                            eprintln!("[ERROR][BACKEND] Failed to write output: {}", e);
                            break;
                        }
                    }
                    Err(e) => {
                        // Output error as markdown content so the viewer shows it
                        let error_content = format!("# Error\n\n{}", e);
                        let session_id = Uuid::new_v4();
                        let encoded = base64::encode_base64(error_content.as_bytes());
                        let chunks = base64::chunk_data(&encoded, MARKDOWN_CHUNK_SIZE);
                        let basedir = compute_basedir(&path);
                        let sequence =
                            osc::generate_markdown_osc(&session_id, chunks, basedir.as_deref());
                        if let Err(e) =
                            output_to_stdout(&super::tmux::passthrough_if_needed(&sequence))
                        {
                            eprintln!("[ERROR][BACKEND] Failed to write error output: {}", e);
                            break;
                        }
                    }
                }
            }

            InteractiveCommand::Image { request_id, path } => {
                let response = generate_image_response(&request_id, &path);
                if let Err(e) = output_to_stdout(&super::tmux::passthrough_if_needed(&response)) {
                    eprintln!("[ERROR][BACKEND] Failed to write image response: {}", e);
                    break;
                }
            }
        }
    }
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

    // --- Existing tests (updated for new signature) ---

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
        // Create a file larger than 2MB but under the 10MB limit -- should succeed
        let large_content = "x".repeat(3 * 1024 * 1024);
        write!(temp_file, "{}", large_content).unwrap();
        temp_file.flush().unwrap();

        let result = execute_markdown_command(temp_file.path());
        assert!(result.is_ok());
    }

    #[test]
    fn test_generate_markdown_output_file_too_large() {
        let mut temp_file = NamedTempFile::new().unwrap();
        // Create a file exceeding the 10MB limit
        let large_content = "x".repeat(11 * 1024 * 1024);
        write!(temp_file, "{}", large_content).unwrap();
        temp_file.flush().unwrap();

        let result = generate_markdown_output(temp_file.path());
        assert!(matches!(result, Err(CommandError::FileTooLarge { .. })));
    }

    #[test]
    fn test_generate_image_response_file_too_large() {
        let mut temp_file = NamedTempFile::with_suffix(".png").unwrap();
        // Create a file exceeding the 50MB limit
        let large_content = vec![0u8; 51 * 1024 * 1024];
        temp_file.write_all(&large_content).unwrap();
        temp_file.flush().unwrap();

        let result = generate_image_response("req-large", temp_file.path());
        assert!(result.contains("image-error"));
        assert!(result.contains("File too large"));
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

    // --- Command parsing tests ---

    #[test]
    fn test_parse_command_quit() {
        assert_eq!(parse_command("quit"), Some(InteractiveCommand::Quit));
        assert_eq!(parse_command("quit\n"), Some(InteractiveCommand::Quit));
        assert_eq!(parse_command("  quit  "), Some(InteractiveCommand::Quit));
    }

    #[test]
    fn test_parse_command_navigate() {
        assert_eq!(
            parse_command("navigate /path/to/file.md"),
            Some(InteractiveCommand::Navigate(PathBuf::from(
                "/path/to/file.md"
            )))
        );
    }

    #[test]
    fn test_parse_command_navigate_with_spaces_in_path() {
        assert_eq!(
            parse_command("navigate /path/to/my file.md"),
            Some(InteractiveCommand::Navigate(PathBuf::from(
                "/path/to/my file.md"
            )))
        );
    }

    #[test]
    fn test_parse_command_navigate_empty_path() {
        assert_eq!(parse_command("navigate "), None);
        assert_eq!(parse_command("navigate"), None);
    }

    #[test]
    fn test_parse_command_image() {
        assert_eq!(
            parse_command("image req123 /path/to/image.png"),
            Some(InteractiveCommand::Image {
                request_id: "req123".to_string(),
                path: PathBuf::from("/path/to/image.png"),
            })
        );
    }

    #[test]
    fn test_parse_command_image_with_spaces_in_path() {
        assert_eq!(
            parse_command("image img-1 /path/to/my image.png"),
            Some(InteractiveCommand::Image {
                request_id: "img-1".to_string(),
                path: PathBuf::from("/path/to/my image.png"),
            })
        );
    }

    #[test]
    fn test_parse_command_image_missing_path() {
        assert_eq!(parse_command("image req123"), None);
        assert_eq!(parse_command("image"), None);
    }

    #[test]
    fn test_parse_command_empty_line() {
        assert_eq!(parse_command(""), None);
        assert_eq!(parse_command("   "), None);
    }

    #[test]
    fn test_parse_command_unknown() {
        assert_eq!(parse_command("unknown command"), None);
        assert_eq!(parse_command("exit"), None);
    }

    // --- MIME type detection tests ---

    #[test]
    fn test_detect_mime_type_png() {
        assert_eq!(detect_mime_type(Path::new("image.png")), Some("image/png"));
    }

    #[test]
    fn test_detect_mime_type_jpg() {
        assert_eq!(detect_mime_type(Path::new("photo.jpg")), Some("image/jpeg"));
    }

    #[test]
    fn test_detect_mime_type_jpeg() {
        assert_eq!(
            detect_mime_type(Path::new("photo.jpeg")),
            Some("image/jpeg")
        );
    }

    #[test]
    fn test_detect_mime_type_gif() {
        assert_eq!(detect_mime_type(Path::new("anim.gif")), Some("image/gif"));
    }

    #[test]
    fn test_detect_mime_type_webp() {
        assert_eq!(
            detect_mime_type(Path::new("image.webp")),
            Some("image/webp")
        );
    }

    #[test]
    fn test_detect_mime_type_svg() {
        assert_eq!(
            detect_mime_type(Path::new("icon.svg")),
            Some("image/svg+xml")
        );
    }

    #[test]
    fn test_detect_mime_type_bmp() {
        assert_eq!(detect_mime_type(Path::new("image.bmp")), Some("image/bmp"));
    }

    #[test]
    fn test_detect_mime_type_ico() {
        assert_eq!(
            detect_mime_type(Path::new("favicon.ico")),
            Some("image/x-icon")
        );
    }

    #[test]
    fn test_detect_mime_type_case_insensitive() {
        assert_eq!(detect_mime_type(Path::new("image.PNG")), Some("image/png"));
        assert_eq!(detect_mime_type(Path::new("photo.JPG")), Some("image/jpeg"));
    }

    #[test]
    fn test_detect_mime_type_unknown_extension() {
        assert_eq!(detect_mime_type(Path::new("file.txt")), None);
        assert_eq!(detect_mime_type(Path::new("file.pdf")), None);
    }

    #[test]
    fn test_detect_mime_type_no_extension() {
        assert_eq!(detect_mime_type(Path::new("noextension")), None);
    }

    // --- Markdown file validation tests ---

    #[test]
    fn test_is_markdown_file_valid() {
        assert!(is_markdown_file(Path::new("file.md")));
        assert!(is_markdown_file(Path::new("/path/to/file.md")));
        assert!(is_markdown_file(Path::new("README.MD")));
    }

    #[test]
    fn test_is_markdown_file_invalid() {
        assert!(!is_markdown_file(Path::new("file.txt")));
        assert!(!is_markdown_file(Path::new("file.markdown")));
        assert!(!is_markdown_file(Path::new("noext")));
    }

    // --- Basedir computation tests ---

    #[test]
    fn test_compute_basedir() {
        assert_eq!(
            compute_basedir(Path::new("/home/user/docs/file.md")),
            Some("/home/user/docs".to_string())
        );
    }

    #[test]
    fn test_compute_basedir_root() {
        assert_eq!(
            compute_basedir(Path::new("/file.md")),
            Some("/".to_string())
        );
    }

    // --- generate_markdown_output tests ---

    #[test]
    fn test_generate_markdown_output_includes_basedir() {
        let mut temp_file = NamedTempFile::new().unwrap();
        writeln!(temp_file, "# Test").unwrap();

        let result = generate_markdown_output(temp_file.path()).unwrap();

        // The output should contain basedir with the temp file's parent directory
        assert!(result.contains("basedir="));
    }

    #[test]
    fn test_generate_markdown_output_not_found() {
        let result = generate_markdown_output(Path::new("/nonexistent/file.md"));
        assert!(result.is_err());
    }

    // --- generate_image_response tests ---

    #[test]
    fn test_generate_image_response_not_found() {
        let result = generate_image_response("req1", Path::new("/nonexistent/image.png"));
        assert!(result.contains("image-error"));
        assert!(result.contains("request_id=req1"));
    }

    #[test]
    fn test_generate_image_response_unsupported_format() {
        let temp_file = NamedTempFile::with_suffix(".txt").unwrap();

        let result = generate_image_response("req2", temp_file.path());
        assert!(result.contains("image-error"));
        assert!(result.contains("request_id=req2"));
        assert!(result.contains("Unsupported image format"));
    }

    #[test]
    fn test_generate_image_response_valid_image() {
        let mut temp_file = NamedTempFile::with_suffix(".png").unwrap();
        // Write minimal PNG header bytes (not a real PNG, but enough to test encoding)
        temp_file.write_all(b"\x89PNG\r\n\x1a\n").unwrap();
        temp_file.flush().unwrap();

        let result = generate_image_response("req3", temp_file.path());
        assert!(result.contains("image-response"));
        assert!(result.contains("request_id=req3"));
        assert!(result.contains("mime_type=image/png"));
        assert!(result.contains("data="));
    }
}
