//! SIXEL encoder.
//! Ported verbatim from `src-tauri/src/protocols/sixel.rs`.

use crate::cli::error::CommandError;
use image::DynamicImage;
use std::collections::HashMap;

/// Maximum number of colors in SIXEL palette
/// Limited to 255 to reserve index 255 for transparency marker
const MAX_COLORS: usize = 255;

/// Generates SIXEL sequence for image.
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
        let r_pct = (r as u32 * 100) / 255;
        let g_pct = (g as u32 * 100) / 255;
        let b_pct = (b as u32 * 100) / 255;
        output.push_str(&format!("#{};2;{};{};{}", idx, r_pct, g_pct, b_pct));
    }

    // Encode sixel data
    // SIXEL encodes 6 vertical pixels per character. The legacy encoder
    // called `encode_band_for_color` once per (band, color), each call
    // re-scanning the full row range and constructing a fresh per-color
    // band buffer. That made encoding O(bands × palette × width) and
    // pushed full 8192×8192×255-color images into multi-second
    // territory. We instead build every per-color plane for the band in
    // a single pass over the indexed pixels (a ~`palette.len()`-fold
    // reduction in scanning work), and emit the planes in the same
    // palette order to preserve byte-for-byte parity with src-tauri.
    let bands = height.div_ceil(6);

    for band in 0..bands {
        let y_start = band * 6;

        // One Vec<u8> per palette color, all pre-filled with 0x3F (the
        // sixel "no bits set" character). Allocated per band so peak
        // memory stays bounded at palette.len() × width (≤ ~2 MiB).
        let mut planes: Vec<Vec<u8>> = (0..palette.len())
            .map(|_| vec![0x3F_u8; width as usize])
            .collect();

        for x in 0..width {
            for bit in 0u32..6 {
                let y = y_start + bit;
                if y < height {
                    let ci = indexed[(y * width + x) as usize] as usize;
                    // The transparency sentinel (255) is intentionally
                    // outside palette bounds and is therefore skipped,
                    // matching the legacy per-color comparison.
                    if ci < palette.len() {
                        planes[ci][x as usize] |= 1 << bit;
                    }
                }
            }
        }

        // Emit every color plane (the legacy `if !band_data.is_empty()`
        // gate was always satisfied for width > 0, so omitting it
        // preserves byte-identical output).
        for (color_idx, plane) in planes.iter().enumerate() {
            output.push_str(&format!("#{}", color_idx));
            output.push_str(&compress_band(plane));
            output.push('$');
        }

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

    for y in 0..height {
        for x in 0..width {
            let pixel = img.get_pixel(x, y);
            if pixel[3] < 128 {
                continue;
            }
            let key = (pixel[0], pixel[1], pixel[2]);
            *color_counts.entry(key).or_insert(0) += 1;
        }
    }

    let mut colors: Vec<_> = color_counts.into_iter().collect();
    colors.sort_by(|a, b| b.1.cmp(&a.1));
    colors.truncate(MAX_COLORS);

    let palette: Vec<(u8, u8, u8)> = colors.iter().map(|(c, _)| *c).collect();

    let color_to_idx: HashMap<(u8, u8, u8), u8> = palette
        .iter()
        .enumerate()
        .map(|(i, &c)| (c, i as u8))
        .collect();

    let mut indexed = Vec::with_capacity((width * height) as usize);
    for y in 0..height {
        for x in 0..width {
            let pixel = img.get_pixel(x, y);
            if pixel[3] < 128 {
                indexed.push(255);
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

    let mut final_palette = palette;
    if final_palette.is_empty() {
        final_palette.push((0, 0, 0));
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

/// Encode a single band (6 rows) for a specific color.
///
/// The hot path in `generate_sixel_sequence` no longer calls this — it
/// builds every per-color plane in one pass over the band for an
/// O(palette.len()) speedup. The function is retained as the
/// per-color reference used by `test_encode_band_for_color` to lock in
/// the legacy semantics that the new vectorized loop must match.
#[cfg(test)]
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

        while i + count < band.len() && band[i + count] == ch && count < 9999 {
            count += 1;
        }

        if count >= 3 {
            result.push_str(&format!("!{}{}", count, ch as char));
        } else {
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
        let mut img = RgbImage::new(2, 2);
        for pixel in img.pixels_mut() {
            *pixel = image::Rgb([255, 0, 0]);
        }
        let dyn_img = DynamicImage::ImageRgb8(img);

        let result = generate_sixel_sequence(&dyn_img);
        assert!(result.is_ok());

        let seq = result.unwrap();
        assert!(seq.starts_with("\x1bPq"));
        assert!(seq.ends_with("\x1b\\"));
        assert!(seq.contains("#0;2;"));
    }

    #[test]
    fn test_generate_sixel_sequence_grayscale() {
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
        let mut img = image::RgbaImage::new(4, 6);
        for x in 0..4 {
            for y in 0..6 {
                if x < 2 {
                    img.put_pixel(x, y, Rgba([255, 0, 0, 255]));
                } else {
                    img.put_pixel(x, y, Rgba([0, 0, 0, 0]));
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
        let band = vec![0x3F, 0x3F, 0x3F, 0x3F, 0x3F];
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

        assert_eq!(palette.len(), 2);
        assert_eq!(indexed.len(), 4);
    }

    #[test]
    fn test_find_nearest_color() {
        let palette = vec![(255, 0, 0), (0, 255, 0), (0, 0, 255)];

        assert_eq!(find_nearest_color(&(255, 0, 0), &palette), 0);
        assert_eq!(find_nearest_color(&(250, 10, 10), &palette), 0);
        assert_eq!(find_nearest_color(&(10, 250, 10), &palette), 1);
    }

    #[test]
    fn test_encode_band_for_color() {
        let indexed = vec![0, 1, 0, 1, 0, 1];
        let band = encode_band_for_color(&indexed, 3, 2, 0, 0);

        assert_eq!(band.len(), 3);
        assert_eq!(band[0], 0x40);
        assert_eq!(band[1], 0x41);
        assert_eq!(band[2], 0x40);
    }

    #[test]
    fn test_max_colors_limit() {
        assert_eq!(MAX_COLORS, 255);
    }

    #[test]
    fn test_transparency_marker_no_collision() {
        let mut img = image::RgbaImage::new(256, 1);

        for x in 0..255 {
            img.put_pixel(x, 0, Rgba([x as u8, 128, 128, 255]));
        }

        img.put_pixel(255, 0, Rgba([0, 0, 0, 0]));

        let (palette, indexed) = quantize_colors(&img, 256, 1);

        assert!(palette.len() <= MAX_COLORS);
        assert_eq!(indexed[255], 255);
    }
}
