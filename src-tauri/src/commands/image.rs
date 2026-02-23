use crate::error::CommandError;
use crate::protocols::{kitty, sixel};
use crate::validation::{file, image as image_validation};
use image::DynamicImage;
use std::io::{self, Read, Write};
use std::path::Path;
use std::time::{Duration, Instant};

/// Maximum file size for image files (10MB)
const MAX_IMAGE_SIZE: u64 = 10 * 1024 * 1024;

/// Maximum image dimensions to prevent decompression bombs
/// 8192x8192 pixels = 256MB for RGBA (reasonable for a terminal image)
const MAX_IMAGE_DIMENSION: u32 = 8192;

#[derive(Debug, Clone, Copy)]
pub enum ImageProtocol {
    Kitty,
    Sixel,
}

impl ImageProtocol {
    /// Parse protocol from string
    /// This is intentionally named differently from FromStr trait to avoid confusion
    pub fn parse(s: &str) -> Result<Self, CommandError> {
        match s {
            "kitty" => Ok(ImageProtocol::Kitty),
            "sixel" => Ok(ImageProtocol::Sixel),
            _ => Err(CommandError::InvalidProtocol(s.to_string())),
        }
    }
}

/// Timeout for waiting for Kitty protocol response
const RESPONSE_TIMEOUT: Duration = Duration::from_secs(30);

/// Executes the image command: reads file, decodes image, generates protocol sequences
pub fn execute_image_command(
    file_path: &Path,
    protocol: ImageProtocol,
) -> Result<(), CommandError> {
    // Open and validate file in one operation (prevents TOCTOU)
    let (mut file, validated_path) = file::open_and_validate_file(file_path, MAX_IMAGE_SIZE)?;

    // Validate image format using magic bytes (not just extension)
    image_validation::validate_image_format(&mut file)?;

    // Decode image (file handle is no longer needed after this)
    drop(file); // Explicitly drop file handle before image::open
    let img = decode_image(&validated_path)?;

    // Generate protocol sequence
    let (sequence, expected_image_id) = match protocol {
        ImageProtocol::Kitty => {
            let (seq, id) = kitty::generate_kitty_sequence(&img)?;
            (seq, Some(id))
        }
        ImageProtocol::Sixel => (sixel::generate_sixel_sequence(&img)?, None),
    };

    // Output to stdout (wrap in DCS passthrough when inside tmux)
    output_to_stdout(&super::tmux::passthrough_if_needed(&sequence))?;

    // Wait for terminal to acknowledge the image (Kitty protocol response)
    // This blocks until the terminal sends back ESC _G ... ESC \ response,
    // preventing the shell prompt from appearing before the image viewer opens.
    //
    // Skip when inside tmux: DCS passthrough is one-directional (process → terminal).
    // The terminal's response cannot travel back through tmux to this process,
    // so waiting would always time out.
    if let Some(image_id) = expected_image_id {
        if !super::tmux::is_inside_tmux() {
            wait_for_kitty_response(image_id);
        }
    }

    Ok(())
}

/// Decodes image from file with dimension checks to prevent decompression bombs
fn decode_image(path: &Path) -> Result<DynamicImage, CommandError> {
    // Check dimensions before full decode to prevent decompression bombs
    let dimensions = image::image_dimensions(path)?;

    if dimensions.0 > MAX_IMAGE_DIMENSION || dimensions.1 > MAX_IMAGE_DIMENSION {
        return Err(CommandError::EncodingError(format!(
            "Image dimensions ({}x{}) exceed maximum allowed ({}x{})",
            dimensions.0, dimensions.1, MAX_IMAGE_DIMENSION, MAX_IMAGE_DIMENSION
        )));
    }

    let img = image::open(path)?;
    Ok(img)
}

/// Waits for Kitty Graphics Protocol response from the terminal.
///
/// The terminal sends back `ESC _G <params> ; OK ESC \` or `ESC _G <params> ; ERROR:<code> ESC \`
/// after processing image data. This function blocks until it receives a response
/// whose `i={id}` matches `expected_id`, or times out.
///
/// If a response with a mismatched id is received, it is ignored and the function
/// continues waiting for the correct response within the timeout period.
fn wait_for_kitty_response(expected_id: u32) {
    // Set stdin to raw mode to read escape sequences byte-by-byte
    let Some(mut raw_guard) = enable_raw_stdin() else {
        return; // Not a terminal or failed to set raw mode
    };

    let mut buf = [0u8; 1];
    let start = Instant::now();

    // State machine to detect ESC _G ... ESC \ (or BEL) pattern
    // States: 0=normal, 1=saw ESC, 2=saw ESC _, 3=saw ESC _G (in response),
    //         4=saw ESC in response (waiting for \)
    let mut state: u8 = 0;
    let mut received = false;
    // Collect response body bytes to parse i={id}
    let mut response_body: Vec<u8> = Vec::new();

    loop {
        if start.elapsed() > RESPONSE_TIMEOUT {
            break;
        }

        match raw_guard.stdin.read(&mut buf) {
            Ok(1) => {
                let b = buf[0];
                state = match (state, b) {
                    (0, 0x1B) => 1, // ESC
                    (1, b'_') => 2, // ESC _
                    (2, b'G') => {
                        // ESC _G - start of response
                        response_body.clear();
                        3
                    }
                    (3, 0x07) => {
                        // BEL - alternative APC terminator
                        if parse_and_match_id(&response_body, expected_id) {
                            received = true;
                            break;
                        }
                        0 // Mismatched id, keep waiting
                    }
                    (3, 0x1B) => {
                        // ESC in response body
                        response_body.push(b);
                        4
                    }
                    (3, _) => {
                        // Continue reading response body (cap at 4KB)
                        if response_body.len() < 4096 {
                            response_body.push(b);
                        }
                        3
                    }
                    (4, b'\\') => {
                        // ESC \ - response complete
                        // Remove the trailing ESC we pushed
                        response_body.pop();
                        if parse_and_match_id(&response_body, expected_id) {
                            received = true;
                            break;
                        }
                        0 // Mismatched id, keep waiting
                    }
                    (4, _) => {
                        // False ESC, continue response
                        response_body.push(b);
                        3
                    }
                    (_, 0x1B) => 1, // ESC in any state restarts detection
                    (_, _) => 0,    // Reset
                };
            }
            Ok(_) => {}
            Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => {
                std::thread::sleep(Duration::from_millis(10));
            }
            Err(_) => break,
        }
    }

    if !received {
        eprintln!("emterm: warning: timed out waiting for terminal response");
    }
}

/// Parse `i={id}` from Kitty response body and check if it matches expected_id.
///
/// Response body format: `i={id};OK` or `i={id};ENOENT:{message}` etc.
/// The params section (before `;`) contains comma-separated key=value pairs.
fn parse_and_match_id(body: &[u8], expected_id: u32) -> bool {
    let body_str = match std::str::from_utf8(body) {
        Ok(s) => s,
        Err(_) => return false,
    };

    // Params are before the semicolon
    let params = body_str.split(';').next().unwrap_or("");

    for param in params.split(',') {
        if let Some(id_str) = param.strip_prefix("i=") {
            if let Ok(id) = id_str.parse::<u32>() {
                return id == expected_id;
            }
        }
    }

    // No i= param found; accept anyway (terminal may not echo id back)
    true
}

/// Signal-safe termios restoration for SIGINT/SIGTERM.
///
/// Ensures the terminal is restored to its original state even if the process
/// is interrupted while stdin is in raw mode. Without this, Ctrl+C during
/// `wait_for_kitty_response()` would leave the terminal in raw mode.
#[cfg(unix)]
mod raw_mode_signal {
    use std::sync::atomic::{AtomicBool, Ordering};

    static ACTIVE: AtomicBool = AtomicBool::new(false);
    // Safety: Written only while ACTIVE is false (before install), read only
    // in signal handler when ACTIVE is true. No concurrent write possible.
    // Uses addr_of!/addr_of_mut! to avoid creating references (Rust 2024).
    static mut SAVED_TERMIOS: libc::termios = unsafe { std::mem::zeroed() };

    extern "C" fn handler(sig: libc::c_int) {
        // All operations here are async-signal-safe (tcsetattr, signal, raise)
        unsafe {
            if ACTIVE.load(Ordering::Acquire) {
                libc::tcsetattr(
                    libc::STDIN_FILENO,
                    libc::TCSANOW,
                    std::ptr::addr_of!(SAVED_TERMIOS),
                );
            }
            // Re-raise with default handler for proper exit behavior
            libc::signal(sig, libc::SIG_DFL);
            libc::raise(sig);
        }
    }

    /// Install signal handlers that restore termios on SIGINT/SIGTERM.
    ///
    /// # Safety
    /// Must be called from a single thread. `original` must be a valid termios.
    pub(super) unsafe fn install(original: &libc::termios) {
        unsafe {
            std::ptr::addr_of_mut!(SAVED_TERMIOS).write(*original);
            ACTIVE.store(true, Ordering::Release);
            libc::signal(libc::SIGINT, handler as *const () as libc::sighandler_t);
            libc::signal(libc::SIGTERM, handler as *const () as libc::sighandler_t);
        }
    }

    /// Remove signal handlers, restoring default behavior.
    ///
    /// # Safety
    /// Must be called from a single thread.
    pub(super) unsafe fn uninstall() {
        unsafe {
            ACTIVE.store(false, Ordering::Release);
            libc::signal(libc::SIGINT, libc::SIG_DFL);
            libc::signal(libc::SIGTERM, libc::SIG_DFL);
        }
    }
}

/// RAII guard for raw stdin mode.
struct RawStdinGuard {
    stdin: io::Stdin,
    #[cfg(unix)]
    original_termios: libc::termios,
}

impl Drop for RawStdinGuard {
    fn drop(&mut self) {
        #[cfg(unix)]
        unsafe {
            libc::tcsetattr(libc::STDIN_FILENO, libc::TCSANOW, &self.original_termios);
            raw_mode_signal::uninstall();
        }
    }
}

/// Enable raw mode on stdin for reading escape sequences.
/// Returns None if stdin is not a terminal.
fn enable_raw_stdin() -> Option<RawStdinGuard> {
    #[cfg(unix)]
    {
        use std::mem::MaybeUninit;

        unsafe {
            // Verify stdin is a terminal (not a pipe or file)
            if libc::isatty(libc::STDIN_FILENO) == 0 {
                return None;
            }

            let mut termios = MaybeUninit::<libc::termios>::uninit();
            if libc::tcgetattr(libc::STDIN_FILENO, termios.as_mut_ptr()) != 0 {
                return None;
            }
            let original = termios.assume_init();

            let mut raw = original;
            // Disable canonical mode and echo
            raw.c_lflag &= !(libc::ICANON | libc::ECHO);
            // Set minimum read to 0 bytes, timeout to 100ms (1 decisecond)
            raw.c_cc[libc::VMIN] = 0;
            raw.c_cc[libc::VTIME] = 1;

            if libc::tcsetattr(libc::STDIN_FILENO, libc::TCSANOW, &raw) != 0 {
                return None;
            }

            // Install signal handlers to restore termios on Ctrl+C / kill
            raw_mode_signal::install(&original);

            Some(RawStdinGuard {
                stdin: io::stdin(),
                original_termios: original,
            })
        }
    }

    #[cfg(not(unix))]
    {
        None
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
    use image::{ImageFormat, RgbaImage};
    use std::io::Write;
    use tempfile::NamedTempFile;

    fn create_test_png() -> NamedTempFile {
        let mut temp_file = NamedTempFile::with_suffix(".png").unwrap();
        let img = RgbaImage::new(10, 10);
        let dyn_img = DynamicImage::ImageRgba8(img);
        dyn_img.write_to(&mut temp_file, ImageFormat::Png).unwrap();
        temp_file
    }

    #[test]
    fn test_image_protocol_parse_kitty() {
        let result = ImageProtocol::parse("kitty");
        assert!(matches!(result, Ok(ImageProtocol::Kitty)));
    }

    #[test]
    fn test_image_protocol_parse_sixel() {
        let result = ImageProtocol::parse("sixel");
        assert!(matches!(result, Ok(ImageProtocol::Sixel)));
    }

    #[test]
    fn test_image_protocol_parse_invalid() {
        let result = ImageProtocol::parse("ascii");
        assert!(matches!(result, Err(CommandError::InvalidProtocol(_))));
    }

    #[test]
    fn test_decode_image_valid_png() {
        let temp_file = create_test_png();
        let result = decode_image(temp_file.path());
        assert!(result.is_ok());
    }

    #[test]
    fn test_execute_image_command_with_valid_png_kitty() {
        let temp_file = create_test_png();
        let result = execute_image_command(temp_file.path(), ImageProtocol::Kitty);
        assert!(result.is_ok());
    }

    #[test]
    fn test_execute_image_command_with_valid_png_sixel() {
        let temp_file = create_test_png();
        let result = execute_image_command(temp_file.path(), ImageProtocol::Sixel);
        // SIXEL is now implemented
        assert!(result.is_ok());
    }

    #[test]
    fn test_execute_image_command_with_non_existent_file() {
        let result =
            execute_image_command(Path::new("/nonexistent/image.png"), ImageProtocol::Kitty);
        assert!(matches!(result, Err(CommandError::FileNotFound(_))));
    }

    #[test]
    fn test_execute_image_command_with_oversized_file() {
        // Create a large dummy file
        let mut temp_file = NamedTempFile::with_suffix(".png").unwrap();
        let large_content = vec![0u8; (11 * 1024 * 1024) as usize]; // 11MB
        temp_file.write_all(&large_content).unwrap();
        temp_file.flush().unwrap();

        let result = execute_image_command(temp_file.path(), ImageProtocol::Kitty);
        assert!(matches!(result, Err(CommandError::FileTooLarge { .. })));
    }

    #[test]
    fn test_decode_image_dimension_check() {
        // Create a small PNG to test dimension validation
        let temp_file = create_test_png();

        // This should succeed as it's a small image
        let result = decode_image(temp_file.path());
        assert!(result.is_ok());
    }

    #[test]
    fn test_max_dimension_constant() {
        // Verify MAX_IMAGE_DIMENSION is set to a reasonable value
        assert_eq!(MAX_IMAGE_DIMENSION, 8192);
    }

    #[test]
    fn test_parse_and_match_id_matching() {
        // Standard OK response with matching id
        assert!(parse_and_match_id(b"i=42;OK", 42));
    }

    #[test]
    fn test_parse_and_match_id_mismatching() {
        // Response with different id
        assert!(!parse_and_match_id(b"i=99;OK", 42));
    }

    #[test]
    fn test_parse_and_match_id_no_id_param() {
        // No i= param - accept (terminal may not echo id)
        assert!(parse_and_match_id(b"OK", 42));
    }

    #[test]
    fn test_parse_and_match_id_error_response() {
        // Error response with matching id
        assert!(parse_and_match_id(b"i=42;ENOENT:not found", 42));
    }

    #[test]
    fn test_parse_and_match_id_multiple_params() {
        // Multiple params in response
        assert!(parse_and_match_id(b"i=42,I=1;OK", 42));
    }

    #[test]
    fn test_parse_and_match_id_empty_body() {
        // Empty body - accept (no id to compare)
        assert!(parse_and_match_id(b"", 42));
    }
}
