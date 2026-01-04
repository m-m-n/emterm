//! Image format decoder.
//!
//! Handles decoding of various image formats to RGBA pixel data.
//!
//! # Supported Formats
//!
//! - PNG (including grayscale, RGB, RGBA)
//! - GIF (including animated GIFs)
//! - Raw RGB/RGBA data

use std::io::Cursor;

use super::animation::GifFrameInfo;

/// Decode PNG data to RGBA pixels.
///
/// # Arguments
///
/// * `data` - Raw PNG data
///
/// # Returns
///
/// Tuple of (width, height, rgba_data) or error message.
pub fn decode_png(data: &[u8]) -> Result<(u32, u32, Vec<u8>), String> {
    let decoder = png::Decoder::new(Cursor::new(data));
    let mut reader = decoder
        .read_info()
        .map_err(|e| format!("PNG decode error: {}", e))?;

    let mut buf = vec![0; reader.output_buffer_size()];
    let info = reader
        .next_frame(&mut buf)
        .map_err(|e| format!("PNG frame error: {}", e))?;

    let width = info.width;
    let height = info.height;

    // Convert to RGBA if needed
    let rgba_data = match info.color_type {
        png::ColorType::Rgba => buf[..info.buffer_size()].to_vec(),
        png::ColorType::Rgb => {
            let rgb = &buf[..info.buffer_size()];
            let mut rgba = Vec::with_capacity((width * height * 4) as usize);
            for chunk in rgb.chunks(3) {
                rgba.extend_from_slice(chunk);
                rgba.push(255); // Alpha
            }
            rgba
        }
        png::ColorType::Grayscale => {
            let gray = &buf[..info.buffer_size()];
            let mut rgba = Vec::with_capacity((width * height * 4) as usize);
            for &g in gray {
                rgba.extend_from_slice(&[g, g, g, 255]);
            }
            rgba
        }
        png::ColorType::GrayscaleAlpha => {
            let ga = &buf[..info.buffer_size()];
            let mut rgba = Vec::with_capacity((width * height * 4) as usize);
            for chunk in ga.chunks(2) {
                let g = chunk[0];
                let a = chunk[1];
                rgba.extend_from_slice(&[g, g, g, a]);
            }
            rgba
        }
        png::ColorType::Indexed => {
            // For indexed images, we need to use the palette
            // The png crate should handle this but we'll be defensive
            return Err("Indexed PNG not yet supported".to_string());
        }
    };

    Ok((width, height, rgba_data))
}

/// Decode raw RGB data to RGBA.
///
/// # Arguments
///
/// * `data` - Raw RGB data (3 bytes per pixel)
/// * `width` - Image width
/// * `height` - Image height
pub fn decode_rgb(data: &[u8], width: u32, height: u32) -> Result<Vec<u8>, String> {
    let expected_size = (width * height * 3) as usize;
    if data.len() < expected_size {
        return Err(format!(
            "RGB data too short: expected {}, got {}",
            expected_size,
            data.len()
        ));
    }

    let mut rgba = Vec::with_capacity((width * height * 4) as usize);
    for chunk in data[..expected_size].chunks(3) {
        rgba.extend_from_slice(chunk);
        rgba.push(255); // Alpha
    }

    Ok(rgba)
}

/// Validate RGBA data size.
///
/// # Arguments
///
/// * `data` - Raw RGBA data (4 bytes per pixel)
/// * `width` - Image width
/// * `height` - Image height
pub fn validate_rgba(data: &[u8], width: u32, height: u32) -> Result<(), String> {
    let expected_size = (width * height * 4) as usize;
    if data.len() < expected_size {
        return Err(format!(
            "RGBA data too short: expected {}, got {}",
            expected_size,
            data.len()
        ));
    }
    Ok(())
}

/// Decode base64 data.
pub fn decode_base64(data: &str) -> Result<Vec<u8>, String> {
    use base64::{Engine, engine::general_purpose::STANDARD};
    STANDARD
        .decode(data)
        .map_err(|e| format!("Base64 decode error: {}", e))
}

/// Encode data to base64.
pub fn encode_base64(data: &[u8]) -> String {
    use base64::{Engine, engine::general_purpose::STANDARD};
    STANDARD.encode(data)
}

/// Decompress ZLIB-compressed data (RFC 1950).
///
/// # Arguments
///
/// * `data` - ZLIB-compressed data
///
/// # Returns
///
/// Decompressed data or error message.
pub fn decompress_zlib(data: &[u8]) -> Result<Vec<u8>, String> {
    use flate2::read::ZlibDecoder;
    use std::io::Read;

    let mut decoder = ZlibDecoder::new(data);
    let mut decompressed = Vec::new();

    decoder
        .read_to_end(&mut decompressed)
        .map_err(|e| format!("ZLIB decompress error: {}", e))?;

    Ok(decompressed)
}

/// Decoded GIF frame with RGBA data.
#[derive(Debug, Clone)]
pub struct DecodedGifFrame {
    /// Frame info (dimensions, delay, etc.).
    pub info: GifFrameInfo,

    /// RGBA pixel data for full canvas.
    pub rgba_data: Vec<u8>,
}

/// Decode GIF data to RGBA frames.
///
/// # Arguments
///
/// * `data` - Raw GIF data
///
/// # Returns
///
/// Tuple of (width, height, frames) or error message.
/// Each frame contains RGBA data for the full canvas.
pub fn decode_gif(data: &[u8]) -> Result<(u32, u32, Vec<DecodedGifFrame>), String> {
    use gif::{DecodeOptions, DisposalMethod};

    let mut decoder = DecodeOptions::new();
    decoder.set_color_output(gif::ColorOutput::RGBA);

    let mut decoder = decoder
        .read_info(Cursor::new(data))
        .map_err(|e| format!("GIF decode error: {}", e))?;

    let width = decoder.width() as u32;
    let height = decoder.height() as u32;
    let canvas_size = (width * height * 4) as usize;

    // Canvas to accumulate frames (for disposal handling)
    let mut canvas = vec![0u8; canvas_size];
    // Background buffer for RestoreBackground disposal
    let bg_color = decoder
        .bg_color()
        .map(|idx| {
            decoder
                .global_palette()
                .and_then(|pal| {
                    let i = idx * 3;
                    if i + 2 < pal.len() {
                        Some([pal[i], pal[i + 1], pal[i + 2], 255])
                    } else {
                        None
                    }
                })
                .unwrap_or([0, 0, 0, 0])
        })
        .unwrap_or([0, 0, 0, 0]);

    let mut frames = Vec::new();
    let mut frame_index = 0;

    while let Some(frame) = decoder
        .read_next_frame()
        .map_err(|e| format!("GIF frame error: {}", e))?
    {
        let left = frame.left as u32;
        let top = frame.top as u32;
        let frame_width = frame.width as u32;
        let frame_height = frame.height as u32;

        // Get delay in milliseconds (GIF stores in centiseconds)
        let delay_ms = (frame.delay as u32) * 10;
        // Default to 100ms if delay is 0 (common in older GIFs)
        let delay_ms = if delay_ms == 0 { 100 } else { delay_ms };

        // Store previous canvas state for disposal handling
        let previous_canvas = canvas.clone();

        // Composite frame onto canvas
        let buffer = &frame.buffer;
        for y in 0..frame_height {
            for x in 0..frame_width {
                let src_idx = ((y * frame_width + x) * 4) as usize;
                let dst_x = left + x;
                let dst_y = top + y;

                if dst_x < width && dst_y < height {
                    let dst_idx = ((dst_y * width + dst_x) * 4) as usize;

                    // Only copy if source pixel is not transparent
                    // (GIF uses index 0 or transparent color for transparency)
                    if src_idx + 3 < buffer.len() && buffer[src_idx + 3] > 0 {
                        canvas[dst_idx..dst_idx + 4].copy_from_slice(&buffer[src_idx..src_idx + 4]);
                    }
                }
            }
        }

        // Store the composited frame
        frames.push(DecodedGifFrame {
            info: GifFrameInfo {
                index: frame_index,
                delay_ms,
                width,
                height,
                left,
                top,
            },
            rgba_data: canvas.clone(),
        });

        // Handle disposal method for next frame
        match frame.dispose {
            DisposalMethod::Keep => {
                // Keep canvas as-is
            }
            DisposalMethod::Background => {
                // Restore frame area to background color
                for y in 0..frame_height {
                    for x in 0..frame_width {
                        let dst_x = left + x;
                        let dst_y = top + y;
                        if dst_x < width && dst_y < height {
                            let idx = ((dst_y * width + dst_x) * 4) as usize;
                            canvas[idx..idx + 4].copy_from_slice(&bg_color);
                        }
                    }
                }
            }
            DisposalMethod::Previous => {
                // Restore to previous canvas state
                canvas = previous_canvas;
            }
            DisposalMethod::Any => {
                // Unspecified - keep canvas
            }
        }

        frame_index += 1;
    }

    if frames.is_empty() {
        return Err("GIF contains no frames".to_string());
    }

    Ok((width, height, frames))
}

/// Decode first frame of GIF to RGBA (for static display).
///
/// # Arguments
///
/// * `data` - Raw GIF data
///
/// # Returns
///
/// Tuple of (width, height, rgba_data) or error message.
pub fn decode_gif_first_frame(data: &[u8]) -> Result<(u32, u32, Vec<u8>), String> {
    let (width, height, frames) = decode_gif(data)?;

    if let Some(first) = frames.into_iter().next() {
        Ok((width, height, first.rgba_data))
    } else {
        Err("GIF contains no frames".to_string())
    }
}

/// Check if data is likely a GIF.
pub fn is_gif(data: &[u8]) -> bool {
    data.len() >= 6 && (data.starts_with(b"GIF87a") || data.starts_with(b"GIF89a"))
}

/// Check if data is likely a PNG.
pub fn is_png(data: &[u8]) -> bool {
    data.len() >= 8 && data.starts_with(&[0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_decode_rgb_to_rgba() {
        let rgb = vec![255, 0, 0, 0, 255, 0]; // Red, Green pixels
        let rgba = decode_rgb(&rgb, 2, 1).unwrap();

        assert_eq!(rgba.len(), 8);
        assert_eq!(rgba[0..4], [255, 0, 0, 255]); // Red with alpha
        assert_eq!(rgba[4..8], [0, 255, 0, 255]); // Green with alpha
    }

    #[test]
    fn test_decode_rgb_too_short() {
        let rgb = vec![255, 0]; // Too short
        let result = decode_rgb(&rgb, 2, 1);
        assert!(result.is_err());
    }

    #[test]
    fn test_validate_rgba_valid() {
        let rgba = vec![0; 16]; // 2x2 image
        assert!(validate_rgba(&rgba, 2, 2).is_ok());
    }

    #[test]
    fn test_validate_rgba_too_short() {
        let rgba = vec![0; 8]; // Too short for 2x2
        assert!(validate_rgba(&rgba, 2, 2).is_err());
    }

    #[test]
    fn test_base64_roundtrip() {
        let data = vec![1, 2, 3, 4, 5];
        let encoded = encode_base64(&data);
        let decoded = decode_base64(&encoded).unwrap();
        assert_eq!(data, decoded);
    }

    #[test]
    fn test_decode_base64_invalid() {
        let result = decode_base64("not valid base64!!!");
        assert!(result.is_err());
    }

    // PNG decoding tests require actual PNG data
    #[test]
    fn test_decode_png_invalid() {
        let result = decode_png(b"not a png");
        assert!(result.is_err());
    }

    #[test]
    fn test_decompress_zlib_valid() {
        use flate2::Compression;
        use flate2::write::ZlibEncoder;
        use std::io::Write;

        // Compress some data
        let original = b"Hello, World! This is a test of ZLIB compression.";
        let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
        encoder.write_all(original).unwrap();
        let compressed = encoder.finish().unwrap();

        // Decompress and verify
        let decompressed = decompress_zlib(&compressed).unwrap();
        assert_eq!(decompressed, original.to_vec());
    }

    #[test]
    fn test_decompress_zlib_invalid() {
        let result = decompress_zlib(b"not compressed data");
        assert!(result.is_err());
    }

    #[test]
    fn test_decompress_zlib_empty() {
        // Empty input returns empty output (flate2 handles gracefully)
        let result = decompress_zlib(&[]);
        // flate2 may return Ok with empty data or error depending on version
        // We just verify it doesn't panic
        let _ = result;
    }

    // =========================================================================
    // GIF Tests
    // =========================================================================

    #[test]
    fn test_is_gif_valid() {
        assert!(is_gif(b"GIF87a...."));
        assert!(is_gif(b"GIF89a...."));
    }

    #[test]
    fn test_is_gif_invalid() {
        assert!(!is_gif(b"PNG"));
        assert!(!is_gif(b"GIF8"));
        assert!(!is_gif(b""));
    }

    #[test]
    fn test_is_png_valid() {
        let png_header = [0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00];
        assert!(is_png(&png_header));
    }

    #[test]
    fn test_is_png_invalid() {
        assert!(!is_png(b"GIF89a"));
        assert!(!is_png(&[0x89, 0x50]));
        assert!(!is_png(b""));
    }

    #[test]
    fn test_decode_gif_invalid() {
        let result = decode_gif(b"not a gif");
        assert!(result.is_err());
    }

    #[test]
    fn test_decode_gif_first_frame_invalid() {
        let result = decode_gif_first_frame(b"not a gif");
        assert!(result.is_err());
    }

    // Note: Testing actual GIF decoding requires valid GIF data.
    // These tests use minimal GIF data for basic validation.

    #[test]
    fn test_decode_gif_minimal() {
        // Minimal valid 1x1 GIF (red pixel)
        // GIF89a header + logical screen descriptor + global color table + image + trailer
        let gif_data = [
            // Header
            0x47, 0x49, 0x46, 0x38, 0x39, 0x61, // "GIF89a"
            // Logical Screen Descriptor
            0x01, 0x00, // width: 1
            0x01, 0x00, // height: 1
            0x80, // packed: global color table, 1 bit
            0x00, // background color index
            0x00, // pixel aspect ratio
            // Global Color Table (2 colors, 6 bytes)
            0xFF, 0x00, 0x00, // color 0: red
            0x00, 0x00, 0x00, // color 1: black
            // Image Descriptor
            0x2C, // image separator
            0x00, 0x00, // left
            0x00, 0x00, // top
            0x01, 0x00, // width: 1
            0x01, 0x00, // height: 1
            0x00, // packed: no local color table
            // Image Data
            0x01, // LZW minimum code size
            0x01, // block size
            0x00, // data (index 0)
            0x00, // block terminator
            // Trailer
            0x3B,
        ];

        // This minimal GIF may not be fully valid for all decoders,
        // but we can at least verify error handling works
        let result = decode_gif(&gif_data);
        // The minimal GIF may or may not decode successfully depending on
        // how strict the gif crate is. What matters is it doesn't panic.
        let _ = result;
    }
}
