use crate::encoding::base64;
use crate::error::CommandError;
use image::{DynamicImage, ImageFormat};
use std::io::Cursor;
use std::sync::atomic::{AtomicU32, Ordering};

/// Chunk size for Kitty Graphics Protocol (4096 bytes)
const KITTY_CHUNK_SIZE: usize = 4096;

/// Process-wide atomic counter for unique Kitty image_id generation.
/// Starts at 1 (id=0 is reserved/invalid). Wraps around skipping 0.
static NEXT_IMAGE_ID: AtomicU32 = AtomicU32::new(1);

/// Generate a unique image_id for a Kitty graphics transfer.
/// Each call returns a different ID. Skips 0 on wrap-around.
fn next_image_id() -> u32 {
    loop {
        let id = NEXT_IMAGE_ID.fetch_add(1, Ordering::Relaxed);
        if id != 0 {
            return id;
        }
        // Wrapped to 0, skip it
    }
}

/// Generates Kitty Graphics Protocol sequence for image
///
/// # Format
/// ```text
/// ESC _G i=1,f=100,a=T,m=1 ; {base64-chunk-1} ESC \
/// ESC _G i=1,m=1 ; {base64-chunk-2} ESC \
/// ...
/// ESC _G i=1,m=0 ; {base64-chunk-last} ESC \
/// ```
///
/// Parameters:
/// - i=1: Image ID (required for chunked transfer)
/// - f=100: PNG format
/// - a=T: Transmit and display
/// - m=1: More data follows
/// - m=0: Last chunk
///
/// Note: q=1 (quiet mode) is intentionally NOT set so the terminal
/// sends back an OK/ERROR response. The `emterm image` CLI command
/// reads this response from stdin to block until processing completes.
pub fn generate_kitty_sequence(img: &DynamicImage) -> Result<(String, u32), CommandError> {
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

    // Pre-allocate: base64 data + ~30 bytes per chunk for escape sequence headers
    let mut output = String::with_capacity(encoded.len() + chunks.len() * 30);

    // Generate unique image_id for this transfer
    let image_id = next_image_id();

    // First chunk with metadata
    // No q=1: allow OK response so CLI can block until processing completes
    output.push_str(&format!(
        "\x1b_Gi={},f=100,a=T,m={};{}\x1b\\",
        image_id,
        if chunks.len() > 1 { 1 } else { 0 },
        String::from_utf8_lossy(chunks[0])
    ));

    // Middle and last chunks (if more than one chunk)
    if chunks.len() > 1 {
        // Middle chunks
        for chunk in &chunks[1..chunks.len() - 1] {
            output.push_str(&format!(
                "\x1b_Gi={},m=1;{}\x1b\\",
                image_id,
                String::from_utf8_lossy(chunk)
            ));
        }

        // Last chunk
        output.push_str(&format!(
            "\x1b_Gi={},m=0;{}\x1b\\",
            image_id,
            String::from_utf8_lossy(chunks[chunks.len() - 1])
        ));
    }

    Ok((output, image_id))
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

        let (sequence, image_id) = result.unwrap();
        // Should have ESC _G prefix
        assert!(sequence.starts_with("\x1b_G"));
        // Should contain the returned image_id
        assert!(sequence.contains(&format!("i={}", image_id)));
        // Should have f=100 (PNG format)
        assert!(sequence.contains("f=100"));
        // Should have a=T (transmit and display)
        assert!(sequence.contains("a=T"));
        // Should NOT have q=1 (responses enabled for CLI blocking)
        assert!(!sequence.contains("q=1"));
        // Small image should fit in one chunk (m=0)
        assert!(sequence.contains("m=0"));
        // image_id must never be 0
        assert_ne!(image_id, 0);
    }

    #[test]
    fn test_generate_kitty_sequence_contains_base64() {
        let img = DynamicImage::ImageRgba8(RgbaImage::new(10, 10));

        let result = generate_kitty_sequence(&img);
        assert!(result.is_ok());

        let (sequence, _) = result.unwrap();
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

    #[test]
    fn test_unique_image_ids() {
        // Each call to next_image_id should return a unique value
        let id1 = next_image_id();
        let id2 = next_image_id();
        let id3 = next_image_id();
        assert_ne!(id1, id2);
        assert_ne!(id2, id3);
        assert_ne!(id1, id3);
        // All must be non-zero
        assert_ne!(id1, 0);
        assert_ne!(id2, 0);
        assert_ne!(id3, 0);
    }

    #[test]
    fn test_consecutive_sequences_have_different_ids() {
        let img = DynamicImage::ImageRgba8(RgbaImage::new(2, 2));
        let (_, id1) = generate_kitty_sequence(&img).unwrap();
        let (_, id2) = generate_kitty_sequence(&img).unwrap();
        assert_ne!(id1, id2);
    }
}
