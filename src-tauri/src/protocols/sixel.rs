use crate::error::CommandError;
use image::DynamicImage;
use std::collections::HashMap;

/// Maximum number of colors in SIXEL palette
/// Limited to 255 to reserve index 255 for transparency marker
const MAX_COLORS: usize = 255;

/// Generates SIXEL sequence for image
///
/// # Format
/// ```text
/// ESC P q {color-definitions} {sixel-data} ESC \
/// ```
pub fn generate_sixel_sequence(img: &DynamicImage) -> Result<String, CommandError> {
    let rgba_img = img.to_rgba8();
    let (width, height) = rgba_img.dimensions();

    if width == 0 || height == 0 {
        return Err(CommandError::EncodingError(
            "Image has zero dimensions".to_string(),
        ));
    }

    // Quantize colors to palette
    let (palette, indexed) = quantize_colors(&rgba_img, width, height);

    // Build SIXEL sequence
    let mut output = String::new();

    // SIXEL introducer: ESC P q
    output.push_str("\x1bPq");

    // Color definitions
    for (idx, &(r, g, b)) in palette.iter().enumerate() {
        // Convert to percentage (0-100)
        let r_pct = (r as u32 * 100) / 255;
        let g_pct = (g as u32 * 100) / 255;
        let b_pct = (b as u32 * 100) / 255;
        output.push_str(&format!("#{};2;{};{};{}", idx, r_pct, g_pct, b_pct));
    }

    // Encode sixel data
    // SIXEL encodes 6 vertical pixels per character
    let bands = height.div_ceil(6);

    for band in 0..bands {
        let y_start = band * 6;

        // For each color in palette, encode the pixels that use that color
        for (color_idx, _) in palette.iter().enumerate() {
            let band_data =
                encode_band_for_color(&indexed, width, height, y_start, color_idx as u8);

            if !band_data.is_empty() {
                // Select color
                output.push_str(&format!("#{}", color_idx));
                // Add band data with RLE
                output.push_str(&compress_band(&band_data));
                // Carriage return to start of band
                output.push('$');
            }
        }

        // Move to next band (graphics newline)
        if band < bands - 1 {
            output.push('-');
        }
    }

    // SIXEL terminator: ESC \
    output.push_str("\x1b\\");

    Ok(output)
}

/// Quantize image colors to a palette of up to MAX_COLORS
fn quantize_colors(
    img: &image::RgbaImage,
    width: u32,
    height: u32,
) -> (Vec<(u8, u8, u8)>, Vec<u8>) {
    let mut color_counts: HashMap<(u8, u8, u8), usize> = HashMap::new();

    // Count color occurrences (ignoring alpha for simplicity)
    for y in 0..height {
        for x in 0..width {
            let pixel = img.get_pixel(x, y);
            // Skip fully transparent pixels
            if pixel[3] < 128 {
                continue;
            }
            let key = (pixel[0], pixel[1], pixel[2]);
            *color_counts.entry(key).or_insert(0) += 1;
        }
    }

    // Sort colors by frequency and take top MAX_COLORS
    let mut colors: Vec<_> = color_counts.into_iter().collect();
    colors.sort_by(|a, b| b.1.cmp(&a.1));
    colors.truncate(MAX_COLORS);

    let palette: Vec<(u8, u8, u8)> = colors.iter().map(|(c, _)| *c).collect();

    // Create color index lookup
    let color_to_idx: HashMap<(u8, u8, u8), u8> = palette
        .iter()
        .enumerate()
        .map(|(i, &c)| (c, i as u8))
        .collect();

    // Create indexed image
    let mut indexed = Vec::with_capacity((width * height) as usize);
    for y in 0..height {
        for x in 0..width {
            let pixel = img.get_pixel(x, y);
            if pixel[3] < 128 {
                // Transparent pixel - use color 0 or special marker
                indexed.push(255); // Marker for transparent
            } else {
                let key = (pixel[0], pixel[1], pixel[2]);
                let idx = color_to_idx
                    .get(&key)
                    .copied()
                    .unwrap_or_else(|| find_nearest_color(&key, &palette));
                indexed.push(idx);
            }
        }
    }

    // Add transparent color if not in palette
    let mut final_palette = palette;
    if final_palette.is_empty() {
        final_palette.push((0, 0, 0)); // Default black
    }

    (final_palette, indexed)
}

/// Find nearest color in palette (simple Euclidean distance)
fn find_nearest_color(color: &(u8, u8, u8), palette: &[(u8, u8, u8)]) -> u8 {
    let mut best_idx = 0u8;
    let mut best_dist = u32::MAX;

    for (idx, &(r, g, b)) in palette.iter().enumerate() {
        let dr = (color.0 as i32 - r as i32).unsigned_abs();
        let dg = (color.1 as i32 - g as i32).unsigned_abs();
        let db = (color.2 as i32 - b as i32).unsigned_abs();
        let dist = dr * dr + dg * dg + db * db;

        if dist < best_dist {
            best_dist = dist;
            best_idx = idx as u8;
        }
    }

    best_idx
}

/// Encode a single band (6 rows) for a specific color
fn encode_band_for_color(
    indexed: &[u8],
    width: u32,
    height: u32,
    y_start: u32,
    color_idx: u8,
) -> Vec<u8> {
    let mut band = Vec::with_capacity(width as usize);

    for x in 0..width {
        let mut sixel_char: u8 = 0;

        for bit in 0..6 {
            let y = y_start + bit;
            if y < height {
                let pixel_idx = (y * width + x) as usize;
                if indexed[pixel_idx] == color_idx {
                    sixel_char |= 1 << bit;
                }
            }
        }

        // SIXEL characters start at 0x3F ('?')
        band.push(0x3F + sixel_char);
    }

    band
}

/// Compress band data using RLE (repeat codes)
fn compress_band(band: &[u8]) -> String {
    if band.is_empty() {
        return String::new();
    }

    let mut result = String::new();
    let mut i = 0;

    while i < band.len() {
        let ch = band[i];
        let mut count = 1;

        // Count consecutive identical characters
        while i + count < band.len() && band[i + count] == ch && count < 9999 {
            count += 1;
        }

        if count >= 3 {
            // Use repeat code for 3+ consecutive
            result.push_str(&format!("!{}{}", count, ch as char));
        } else {
            // Output individual characters
            for _ in 0..count {
                result.push(ch as char);
            }
        }

        i += count;
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{RgbImage, Rgba};

    #[test]
    fn test_generate_sixel_sequence_small_image() {
        // Create a simple 2x2 red image
        let mut img = RgbImage::new(2, 2);
        for pixel in img.pixels_mut() {
            *pixel = image::Rgb([255, 0, 0]);
        }
        let dyn_img = DynamicImage::ImageRgb8(img);

        let result = generate_sixel_sequence(&dyn_img);
        assert!(result.is_ok());

        let seq = result.unwrap();
        // Should start with SIXEL introducer
        assert!(seq.starts_with("\x1bPq"));
        // Should end with SIXEL terminator
        assert!(seq.ends_with("\x1b\\"));
        // Should contain color definition
        assert!(seq.contains("#0;2;"));
    }

    #[test]
    fn test_generate_sixel_sequence_grayscale() {
        // Create a grayscale gradient
        let mut img = RgbImage::new(10, 6);
        for x in 0..10 {
            let gray = (x * 25) as u8;
            for y in 0..6 {
                img.put_pixel(x, y, image::Rgb([gray, gray, gray]));
            }
        }
        let dyn_img = DynamicImage::ImageRgb8(img);

        let result = generate_sixel_sequence(&dyn_img);
        assert!(result.is_ok());
    }

    #[test]
    fn test_generate_sixel_sequence_with_transparency() {
        // Create RGBA image with some transparent pixels
        let mut img = image::RgbaImage::new(4, 6);
        for x in 0..4 {
            for y in 0..6 {
                if x < 2 {
                    img.put_pixel(x, y, Rgba([255, 0, 0, 255])); // Opaque red
                } else {
                    img.put_pixel(x, y, Rgba([0, 0, 0, 0])); // Transparent
                }
            }
        }
        let dyn_img = DynamicImage::ImageRgba8(img);

        let result = generate_sixel_sequence(&dyn_img);
        assert!(result.is_ok());
    }

    #[test]
    fn test_compress_band_no_repeats() {
        let band = vec![0x3F, 0x40, 0x41, 0x42];
        let compressed = compress_band(&band);
        assert_eq!(compressed, "?@AB");
    }

    #[test]
    fn test_compress_band_with_repeats() {
        let band = vec![0x3F, 0x3F, 0x3F, 0x3F, 0x3F]; // 5 '?'
        let compressed = compress_band(&band);
        assert_eq!(compressed, "!5?");
    }

    #[test]
    fn test_compress_band_mixed() {
        let band = vec![0x3F, 0x40, 0x40, 0x40, 0x40, 0x41];
        let compressed = compress_band(&band);
        assert_eq!(compressed, "?!4@A");
    }

    #[test]
    fn test_quantize_colors_simple() {
        let mut img = image::RgbaImage::new(2, 2);
        img.put_pixel(0, 0, Rgba([255, 0, 0, 255]));
        img.put_pixel(1, 0, Rgba([255, 0, 0, 255]));
        img.put_pixel(0, 1, Rgba([0, 255, 0, 255]));
        img.put_pixel(1, 1, Rgba([0, 255, 0, 255]));

        let (palette, indexed) = quantize_colors(&img, 2, 2);

        // Should have 2 colors
        assert_eq!(palette.len(), 2);
        // Should have 4 indexed pixels
        assert_eq!(indexed.len(), 4);
    }

    #[test]
    fn test_find_nearest_color() {
        let palette = vec![(255, 0, 0), (0, 255, 0), (0, 0, 255)];

        // Exact match
        assert_eq!(find_nearest_color(&(255, 0, 0), &palette), 0);

        // Close to red
        assert_eq!(find_nearest_color(&(250, 10, 10), &palette), 0);

        // Close to green
        assert_eq!(find_nearest_color(&(10, 250, 10), &palette), 1);
    }

    #[test]
    fn test_encode_band_for_color() {
        // 3 pixels wide, 2 rows: indexed as row-major [row0: 0,1,0] [row1: 1,0,1]
        let indexed = vec![0, 1, 0, 1, 0, 1]; // 3x2 image
        let band = encode_band_for_color(&indexed, 3, 2, 0, 0);

        assert_eq!(band.len(), 3);
        // First column (x=0): color 0 at row 0 -> bit 0 set -> 0x3F + 1 = 0x40
        assert_eq!(band[0], 0x40);
        // Second column (x=1): color 0 at row 1 -> bit 1 set -> 0x3F + 2 = 0x41
        assert_eq!(band[1], 0x41);
        // Third column (x=2): color 0 at row 0 -> bit 0 set -> 0x40
        assert_eq!(band[2], 0x40);
    }

    #[test]
    fn test_max_colors_limit() {
        // Verify MAX_COLORS is limited to 255 to reserve index 255 for transparency
        assert_eq!(MAX_COLORS, 255);
    }

    #[test]
    fn test_transparency_marker_no_collision() {
        // Create an image with many colors to ensure transparency marker (255) doesn't collide
        let mut img = image::RgbaImage::new(256, 1);

        // Fill with 255 different colors
        for x in 0..255 {
            img.put_pixel(x, 0, Rgba([x as u8, 128, 128, 255]));
        }

        // Add one transparent pixel
        img.put_pixel(255, 0, Rgba([0, 0, 0, 0]));

        let (palette, indexed) = quantize_colors(&img, 256, 1);

        // Palette should have at most 255 colors (not 256) to reserve 255 for transparency
        assert!(palette.len() <= MAX_COLORS);

        // The transparent pixel should be marked with 255
        assert_eq!(indexed[255], 255);
    }
}
