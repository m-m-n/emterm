//! `image` subcommand handler.
//!
//! Ported from `src-tauri/src/commands/image.rs`. The Unix-only stdin
//! drain is cfg-gated to `unix`; Windows uses a no-op stub.

use crate::cli::error::CommandError;
use crate::cli::protocols::{kitty, sixel};
use crate::cli::tmux;
use crate::cli::validation::{file, image as image_validation};
use image::DynamicImage;
use std::io::{self, Cursor, Read, Seek, SeekFrom, Write};
use std::path::Path;

/// Maximum file size for image files (10MB)
const MAX_IMAGE_SIZE: u64 = 10 * 1024 * 1024;

/// Maximum image dimensions to prevent decompression bombs.
/// 8192x8192 pixels = 256MB for RGBA (reasonable for a terminal image)
const MAX_IMAGE_DIMENSION: u32 = 8192;

#[derive(Debug, Clone, Copy)]
pub enum ImageProtocol {
    Kitty,
    Sixel,
}

impl ImageProtocol {
    /// Parse protocol from string. Named differently from `FromStr` to
    /// avoid trait-vs-method confusion in `cli::run`.
    pub fn parse(s: &str) -> Result<Self, CommandError> {
        match s {
            "kitty" => Ok(ImageProtocol::Kitty),
            "sixel" => Ok(ImageProtocol::Sixel),
            _ => Err(CommandError::InvalidProtocol(s.to_string())),
        }
    }
}

/// Executes the image command: reads file, decodes image, generates protocol sequences.
pub fn execute_image_command(
    file_path: &Path,
    protocol: ImageProtocol,
) -> Result<(), CommandError> {
    let (mut file, _validated_path) = file::open_and_validate_file(file_path, MAX_IMAGE_SIZE)?;

    image_validation::validate_image_format(&mut file)?;

    // Read the validated fd into memory so all subsequent operations
    // (dimension probe + decode) work on the bytes we already verified.
    // This closes the TOCTOU window between size-check on the fd and
    // re-opening by path that the previous `image::image_dimensions` +
    // `image::open` pair had.
    file.seek(SeekFrom::Start(0))?;
    let mut buf = Vec::new();
    file.read_to_end(&mut buf)?;
    drop(file);

    let img = decode_image_from_bytes(&buf)?;

    let sequence = match protocol {
        ImageProtocol::Kitty => {
            let (seq, _id) = kitty::generate_kitty_sequence(&img)?;
            seq
        }
        ImageProtocol::Sixel => sixel::generate_sixel_sequence(&img)?,
    };

    output_to_stdout(&tmux::passthrough_if_needed(&sequence))?;

    #[cfg(unix)]
    drain_stdin_responses();

    Ok(())
}

/// Decodes image from an in-memory byte buffer with dimension checks
/// to prevent decompression bombs. Reading the file once into `buf` (in
/// `execute_image_command`) closes the TOCTOU race that
/// `image::image_dimensions(path)` + `image::open(path)` had.
fn decode_image_from_bytes(buf: &[u8]) -> Result<DynamicImage, CommandError> {
    let reader = image::ImageReader::new(Cursor::new(buf)).with_guessed_format()?;
    let (w, h) = reader.into_dimensions()?;

    if w > MAX_IMAGE_DIMENSION || h > MAX_IMAGE_DIMENSION {
        return Err(CommandError::EncodingError(format!(
            "Image dimensions ({}x{}) exceed maximum allowed ({}x{})",
            w, h, MAX_IMAGE_DIMENSION, MAX_IMAGE_DIMENSION
        )));
    }

    let img = image::load_from_memory(buf)?;
    Ok(img)
}

/// Drain any Kitty APC response bytes from stdin after sequence output.
///
/// Some terminals send OK responses despite q=2 (suppress). If these
/// bytes remain in stdin, the shell interprets them as user input,
/// causing garbage text on the prompt (e.g., `Gi=1;OK`).
#[cfg(unix)]
fn drain_stdin_responses() {
    use std::io::Read;
    use std::os::unix::io::AsRawFd;

    let stdin_fd = std::io::stdin().as_raw_fd();

    let orig = unsafe {
        let mut termios = std::mem::zeroed::<libc::termios>();
        if libc::tcgetattr(stdin_fd, &mut termios) != 0 {
            return;
        }
        termios
    };

    let mut raw = orig;
    unsafe {
        libc::cfmakeraw(&mut raw);
        // VMIN=0: return immediately if no data, VTIME=20: 2s timeout
        raw.c_cc[libc::VMIN] = 0;
        raw.c_cc[libc::VTIME] = 20;
        if libc::tcsetattr(stdin_fd, libc::TCSANOW, &raw) != 0 {
            return;
        }
    }

    let mut buf = [0u8; 256];
    loop {
        match std::io::stdin().lock().read(&mut buf) {
            Ok(0) => break,
            Ok(n) if n < buf.len() => break,
            Ok(_) => continue,
            Err(_) => break,
        }
    }

    unsafe {
        libc::tcsetattr(stdin_fd, libc::TCSANOW, &orig);
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
        let bytes = std::fs::read(temp_file.path()).unwrap();
        let result = decode_image_from_bytes(&bytes);
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
        let mut temp_file = NamedTempFile::with_suffix(".png").unwrap();
        let large_content = vec![0u8; (11 * 1024 * 1024) as usize];
        temp_file.write_all(&large_content).unwrap();
        temp_file.flush().unwrap();

        let result = execute_image_command(temp_file.path(), ImageProtocol::Kitty);
        assert!(matches!(result, Err(CommandError::FileTooLarge { .. })));
    }

    #[test]
    fn test_decode_image_dimension_check() {
        let temp_file = create_test_png();
        let bytes = std::fs::read(temp_file.path()).unwrap();
        let result = decode_image_from_bytes(&bytes);
        assert!(result.is_ok());
    }

    #[test]
    fn test_max_dimension_constant() {
        assert_eq!(MAX_IMAGE_DIMENSION, 8192);
    }

    #[test]
    fn test_max_size_constant() {
        assert_eq!(MAX_IMAGE_SIZE, 10 * 1024 * 1024);
    }
}
