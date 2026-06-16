//! Image-format validation via magic bytes.
//! Ported from `src-tauri/src/validation/image.rs`.

use crate::cli::error::CommandError;
use image::ImageFormat;
use std::fs::File;
use std::io::Read;
use std::path::Path;

/// Validates image format is supported by reading magic bytes (not just extension)
pub fn validate_image_format(file: &mut File) -> Result<ImageFormat, CommandError> {
    let mut buffer = [0u8; 16];
    let bytes_read = file.read(&mut buffer)?;

    let format = image::guess_format(&buffer[..bytes_read])?;

    use std::io::Seek;
    file.seek(std::io::SeekFrom::Start(0))?;

    match format {
        ImageFormat::Png | ImageFormat::Jpeg | ImageFormat::Gif | ImageFormat::WebP => Ok(format),
        _ => Err(CommandError::UnsupportedImageFormat(format)),
    }
}

/// Validates image format from path (for compatibility, but prefer using File handle)
pub fn validate_image_format_from_path(path: &Path) -> Result<ImageFormat, CommandError> {
    let mut file = File::open(path)?;
    validate_image_format(&mut file)
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{ImageFormat as ImgFormat, RgbImage};
    use std::io::Write;
    use tempfile::NamedTempFile;

    fn create_test_image_file(format: ImgFormat) -> NamedTempFile {
        let mut temp_file = NamedTempFile::new().unwrap();
        let img = image::DynamicImage::ImageRgb8(RgbImage::new(2, 2));
        img.write_to(&mut temp_file, format).unwrap();
        temp_file.flush().unwrap();
        temp_file
    }

    #[test]
    fn test_validate_image_format_png() {
        let temp_file = create_test_image_file(ImgFormat::Png);
        let result = validate_image_format_from_path(temp_file.path());
        assert!(matches!(result, Ok(ImageFormat::Png)));
    }

    #[test]
    fn test_validate_image_format_jpeg() {
        let temp_file = create_test_image_file(ImgFormat::Jpeg);
        let result = validate_image_format_from_path(temp_file.path());
        assert!(matches!(result, Ok(ImageFormat::Jpeg)));
    }

    #[test]
    fn test_validate_image_format_gif() {
        let temp_file = create_test_image_file(ImgFormat::Gif);
        let result = validate_image_format_from_path(temp_file.path());
        assert!(matches!(result, Ok(ImageFormat::Gif)));
    }

    #[test]
    fn test_validate_image_format_webp() {
        let temp_file = create_test_image_file(ImgFormat::WebP);
        let result = validate_image_format_from_path(temp_file.path());
        assert!(matches!(result, Ok(ImageFormat::WebP)));
    }

    #[test]
    fn test_validate_image_format_invalid_data() {
        let mut temp_file = NamedTempFile::new().unwrap();
        temp_file.write_all(b"This is not an image").unwrap();
        temp_file.flush().unwrap();

        let result = validate_image_format_from_path(temp_file.path());
        assert!(result.is_err());
    }

    #[test]
    fn test_validate_image_format_spoofed_extension() {
        let mut temp_file = NamedTempFile::with_suffix(".png").unwrap();
        let img = image::DynamicImage::ImageRgb8(RgbImage::new(2, 2));
        img.write_to(&mut temp_file, ImgFormat::Jpeg).unwrap();
        temp_file.flush().unwrap();

        let result = validate_image_format_from_path(temp_file.path());
        assert!(matches!(result, Ok(ImageFormat::Jpeg)));
    }
}
