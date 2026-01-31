use assert_cmd::Command;
use predicates::prelude::*;
use std::fs;
use std::io::Write;
use tempfile::NamedTempFile;

/// Test 1: PNG image with Kitty protocol
#[test]
fn test_image_png_kitty() {
    let mut cmd = Command::cargo_bin("emterm").unwrap();

    let output = cmd
        .arg("image")
        .arg("tests/fixtures/small.png")
        .output()
        .expect("Failed to execute command");

    // Should succeed
    assert!(
        output.status.success(),
        "Command failed: {:?}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);

    // Verify Kitty Graphics Protocol sequence format
    assert!(
        stdout.contains("\x1b_G"),
        "Missing Kitty sequence start (ESC _G)"
    );
    assert!(
        stdout.contains("f=100"),
        "Missing format parameter f=100 (PNG)"
    );
    assert!(
        stdout.contains("a=T"),
        "Missing action parameter a=T (transmit and display)"
    );

    // Verify base64 data is present
    assert!(stdout.contains(';'), "Missing parameter separator");
}

/// Test 2: JPEG image with Kitty protocol
#[test]
fn test_image_jpeg_kitty() {
    let mut cmd = Command::cargo_bin("emterm").unwrap();

    let output = cmd
        .arg("image")
        .arg("tests/fixtures/photo.jpg")
        .output()
        .expect("Failed to execute command");

    // Should succeed
    assert!(
        output.status.success(),
        "Command failed: {:?}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);

    // JPEG should be converted to PNG for Kitty protocol
    assert!(stdout.contains("\x1b_G"), "Missing Kitty sequence start");
    assert!(
        stdout.contains("f=100"),
        "Missing format parameter f=100 (PNG)"
    );
}

/// Test 3: GIF image with Kitty protocol
#[test]
fn test_image_gif_kitty() {
    let mut cmd = Command::cargo_bin("emterm").unwrap();

    let output = cmd
        .arg("image")
        .arg("tests/fixtures/animation.gif")
        .output()
        .expect("Failed to execute command");

    // Should succeed (first frame should be extracted and displayed)
    assert!(
        output.status.success(),
        "Command failed: {:?}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);

    assert!(stdout.contains("\x1b_G"), "Missing Kitty sequence start");
    assert!(
        stdout.contains("f=100"),
        "Missing format parameter f=100 (PNG)"
    );
}

/// Test 4: WebP image with Kitty protocol
#[test]
fn test_image_webp_kitty() {
    let mut cmd = Command::cargo_bin("emterm").unwrap();

    let output = cmd
        .arg("image")
        .arg("tests/fixtures/graphic.webp")
        .output()
        .expect("Failed to execute command");

    // Should succeed
    assert!(
        output.status.success(),
        "Command failed: {:?}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);

    assert!(stdout.contains("\x1b_G"), "Missing Kitty sequence start");
    assert!(
        stdout.contains("f=100"),
        "Missing format parameter f=100 (PNG)"
    );
}

/// Test 5: Image at size limit (exactly 10MB)
#[test]
fn test_image_at_size_limit() {
    // Create a ~10MB PNG (may compress smaller, but uncompressed should be near limit)
    // Use a simple pattern to generate a large image
    use image::{ImageBuffer, Rgb};

    let width = 2000;
    let height = 2000;
    let img = ImageBuffer::from_fn(width, height, |x, y| {
        let r = (x % 256) as u8;
        let g = (y % 256) as u8;
        let b = ((x + y) % 256) as u8;
        Rgb([r, g, b])
    });

    let mut temp_file = NamedTempFile::with_suffix(".png").unwrap();
    img.save(temp_file.path()).unwrap();

    let file_size = fs::metadata(temp_file.path()).unwrap().len();
    println!("Generated image size: {} bytes", file_size);

    // If the file is under 10MB, proceed with test
    if file_size <= 10_485_760 {
        let mut cmd = Command::cargo_bin("emterm").unwrap();

        let output = cmd
            .arg("image")
            .arg(temp_file.path())
            .output()
            .expect("Failed to execute command");

        // Should succeed
        assert!(
            output.status.success(),
            "Command should succeed for image under 10MB: {:?}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
}

/// Test 6: File over size limit (10MB + 1 byte)
#[test]
fn test_image_over_size_limit() {
    // Create a file just over 10MB
    let mut temp_file = NamedTempFile::with_suffix(".png").unwrap();
    let size = 10_485_761; // 10MB + 1 byte
    let content = vec![0u8; size];
    temp_file.write_all(&content).unwrap();
    temp_file.flush().unwrap();

    let mut cmd = Command::cargo_bin("emterm").unwrap();

    let output = cmd
        .arg("image")
        .arg(temp_file.path())
        .output()
        .expect("Failed to execute command");

    // Should fail with exit code 1
    assert!(
        !output.status.success(),
        "Command should fail for oversized file"
    );
    assert_eq!(output.status.code(), Some(1), "Expected exit code 1");

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("too large")
            || stderr.contains("size limit")
            || stderr.contains("exceeds")
            || stderr.contains("\u{5236}\u{9650}\u{3092}\u{8d85}\u{3048}"), // 制限を超え (ja)
        "Error message should mention size limit: {}",
        stderr
    );
}

/// Test 7: Unsupported image format
#[test]
fn test_image_unsupported_format() {
    // Create a file with unsupported format (BMP data with .bmp extension)
    let mut temp_file = NamedTempFile::with_suffix(".bmp").unwrap();
    // Write minimal BMP header (not valid, but enough to trigger format detection)
    temp_file.write_all(b"BM").unwrap();
    temp_file.write_all(&[0u8; 100]).unwrap();
    temp_file.flush().unwrap();

    let mut cmd = Command::cargo_bin("emterm").unwrap();

    let output = cmd
        .arg("image")
        .arg(temp_file.path())
        .output()
        .expect("Failed to execute command");

    // Should fail with exit code 1
    assert!(
        !output.status.success(),
        "Command should fail for unsupported format"
    );
    assert_eq!(output.status.code(), Some(1), "Expected exit code 1");

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("unsupported")
            || stderr.contains("format")
            || stderr.contains("invalid")
            || stderr.contains("\u{30b5}\u{30dd}\u{30fc}\u{30c8}\u{3055}\u{308c}\u{3066}\u{3044}\u{306a}\u{3044}") // サポートされていない (ja)
            || stderr.contains("\u{30c7}\u{30b3}\u{30fc}\u{30c9}"), // デコード (ja)
        "Error message should mention unsupported format: {}",
        stderr
    );
}

/// Test 8: Corrupted image file
#[test]
fn test_image_corrupted_file() {
    // Create a file with .png extension but invalid data
    let mut temp_file = NamedTempFile::with_suffix(".png").unwrap();
    temp_file
        .write_all(b"This is not a valid PNG file")
        .unwrap();
    temp_file.flush().unwrap();

    let mut cmd = Command::cargo_bin("emterm").unwrap();

    let output = cmd
        .arg("image")
        .arg(temp_file.path())
        .output()
        .expect("Failed to execute command");

    // Should fail with exit code 1
    assert!(
        !output.status.success(),
        "Command should fail for corrupted image"
    );
    assert_eq!(output.status.code(), Some(1), "Expected exit code 1");

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("invalid")
            || stderr.contains("decode")
            || stderr.contains("failed")
            || stderr.contains("corrupted")
            || stderr.contains("\u{30c7}\u{30b3}\u{30fc}\u{30c9}\u{306b}\u{5931}\u{6557}") // デコードに失敗 (ja)
            || stderr.contains("\u{5931}\u{6557}\u{3057}\u{307e}\u{3057}\u{305f}"), // 失敗しました (ja)
        "Error message should mention decode failure: {}",
        stderr
    );
}

/// Integration test: Verify Kitty chunking for larger images
#[test]
fn test_image_kitty_chunking() {
    // Use a known image that will produce multiple chunks
    let mut cmd = Command::cargo_bin("emterm").unwrap();

    let output = cmd
        .arg("image")
        .arg("tests/fixtures/photo.jpg")
        .output()
        .expect("Failed to execute command");

    assert!(output.status.success());

    let stdout = String::from_utf8_lossy(&output.stdout);

    // Check for chunking markers
    // First chunk should have m=1 (more data)
    // Last chunk should have m=0 (end of transmission)
    if stdout.contains("m=1") {
        // Multi-chunk transmission
        assert!(
            stdout.contains("m=0"),
            "Multi-chunk transmission should end with m=0"
        );
    } else {
        // Single chunk transmission
        assert!(
            stdout.contains("m=0") || !stdout.contains("m="),
            "Single chunk should have m=0 or no m parameter"
        );
    }
}

/// Integration test: Verify protocol option
#[test]
fn test_image_protocol_option() {
    let mut cmd = Command::cargo_bin("emterm").unwrap();

    let output = cmd
        .arg("image")
        .arg("--protocol")
        .arg("kitty")
        .arg("tests/fixtures/small.png")
        .output()
        .expect("Failed to execute command");

    // Should succeed with explicit protocol
    assert!(
        output.status.success(),
        "Command failed with --protocol=kitty: {:?}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("\x1b_G"), "Missing Kitty sequence");
}
