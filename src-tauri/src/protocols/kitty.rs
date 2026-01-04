use crate::encoding::base64;
use crate::error::CommandError;
use image::{DynamicImage, ImageFormat};
use std::io::Cursor;

/// Chunk size for Kitty Graphics Protocol (4096 bytes)
const KITTY_CHUNK_SIZE: usize = 4096;

/// Generates Kitty Graphics Protocol sequence for image
///
/// # Format
/// ```text
/// ESC _G f=100,a=T,m=1 ; {base64-chunk-1} ESC \
/// ESC _G m=1 ; {base64-chunk-2} ESC \
/// ...
/// ESC _G m=0 ; {base64-chunk-last} ESC \
/// ```
///
/// Parameters:
/// - f=100: PNG format
/// - a=T: Transmit and display
/// - m=1: More data follows
/// - m=0: Last chunk
pub fn generate_kitty_sequence(img: &DynamicImage) -> Result<String, CommandError> {
    // Convert image to PNG bytes
    let mut png_bytes = Vec::new();
    img.write_to(&mut Cursor::new(&mut png_bytes), ImageFormat::Png)
        .map_err(|e| CommandError::EncodingError(format!("Failed to encode PNG: {}", e)))?;

    // Base64 encode
    let encoded = base64::encode_base64(&png_bytes);

    // Split into chunks
    let chunks: Vec<_> = encoded.as_bytes().chunks(KITTY_CHUNK_SIZE).collect();

    if chunks.is_empty() {
        return Err(CommandError::EncodingError("No data to encode".to_string()));
    }

    let mut output = String::new();

    // First chunk with metadata
    output.push_str(&format!(
        "\x1b_Gf=100,a=T,m={};{}\x1b\\",
        if chunks.len() > 1 { 1 } else { 0 },
        String::from_utf8_lossy(chunks[0])
    ));

    // Middle and last chunks (if more than one chunk)
    if chunks.len() > 1 {
        // Middle chunks
        for chunk in &chunks[1..chunks.len() - 1] {
            output.push_str(&format!(
                "\x1b_Gm=1;{}\x1b\\",
                String::from_utf8_lossy(chunk)
            ));
        }

        // Last chunk
        output.push_str(&format!(
            "\x1b_Gm=0;{}\x1b\\",
            String::from_utf8_lossy(chunks[chunks.len() - 1])
        ));
    }

    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::RgbaImage;

    #[test]
    fn test_generate_kitty_sequence_small_image() {
        // Create a tiny 2x2 image
        let img = DynamicImage::ImageRgba8(RgbaImage::new(2, 2));

        let result = generate_kitty_sequence(&img);
        assert!(result.is_ok());

        let sequence = result.unwrap();
        // Should have ESC _G prefix
        assert!(sequence.starts_with("\x1b_G"));
        // Should have f=100 (PNG format)
        assert!(sequence.contains("f=100"));
        // Should have a=T (transmit and display)
        assert!(sequence.contains("a=T"));
        // Small image should fit in one chunk (m=0)
        assert!(sequence.contains("m=0"));
    }

    #[test]
    fn test_generate_kitty_sequence_contains_base64() {
        let img = DynamicImage::ImageRgba8(RgbaImage::new(10, 10));

        let result = generate_kitty_sequence(&img);
        assert!(result.is_ok());

        let sequence = result.unwrap();
        // Should contain base64-like characters after semicolon
        assert!(sequence.contains(";"));
        // Should end with ESC backslash
        assert!(sequence.ends_with("\x1b\\"));
    }

    #[test]
    fn test_kitty_chunk_size_constant() {
        // Verify the chunk size constant matches spec
        assert_eq!(KITTY_CHUNK_SIZE, 4096);
    }
}
