use crate::error::CommandError;
use crate::protocols::{kitty, sixel};
use crate::validation::{file, image as image_validation};
use image::DynamicImage;
use std::io::{self, Write};
use std::path::Path;

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
    let sequence = match protocol {
        ImageProtocol::Kitty => {
            let (seq, _id) = kitty::generate_kitty_sequence(&img)?;
            seq
        }
        ImageProtocol::Sixel => sixel::generate_sixel_sequence(&img)?,
    };

    // Output to stdout (wrap in DCS passthrough when inside tmux).
    // Kitty sequences use q=2 (suppress OK responses), but the hosting
    // terminal may still send APC responses. Drain stdin after output
    // to prevent response bytes from leaking into the shell prompt.
    output_to_stdout(&super::tmux::passthrough_if_needed(&sequence))?;

    #[cfg(unix)]
    drain_stdin_responses();

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

}
