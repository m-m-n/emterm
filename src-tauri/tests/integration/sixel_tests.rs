use assert_cmd::Command;
use predicates::prelude::*;

/// Test 1: PNG image with SIXEL protocol
#[test]
fn test_image_png_sixel() {
    let mut cmd = Command::cargo_bin("emterm").unwrap();

    let output = cmd
        .arg("image")
        .arg("--protocol")
        .arg("sixel")
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

    // Verify SIXEL DCS sequence format
    // SIXEL starts with DCS: ESC P q (or \x1bPq)
    assert!(
        stdout.contains("\x1bPq") || stdout.starts_with("\x1bP"),
        "Missing SIXEL DCS start sequence (ESC P q)"
    );

    // SIXEL ends with ST: ESC \ (or \x1b\\)
    assert!(
        stdout.contains("\x1b\\"),
        "Missing SIXEL sequence terminator (ESC \\)"
    );
}

/// Test 2: JPEG image with SIXEL protocol
#[test]
fn test_image_jpeg_sixel() {
    let mut cmd = Command::cargo_bin("emterm").unwrap();

    let output = cmd
        .arg("image")
        .arg("--protocol")
        .arg("sixel")
        .arg("tests/fixtures/photo.jpg")
        .output()
        .expect("Failed to execute command");

    // Should succeed (JPEG converted and encoded to SIXEL)
    assert!(
        output.status.success(),
        "Command failed: {:?}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);

    // Verify SIXEL sequence
    assert!(
        stdout.contains("\x1bPq") || stdout.starts_with("\x1bP"),
        "Missing SIXEL DCS start sequence"
    );
    assert!(
        stdout.contains("\x1b\\"),
        "Missing SIXEL sequence terminator"
    );
}

/// Test 3: GIF image with SIXEL protocol
#[test]
fn test_image_gif_sixel() {
    let mut cmd = Command::cargo_bin("emterm").unwrap();

    let output = cmd
        .arg("image")
        .arg("--protocol")
        .arg("sixel")
        .arg("tests/fixtures/animation.gif")
        .output()
        .expect("Failed to execute command");

    // Should succeed (first frame extracted and encoded)
    assert!(
        output.status.success(),
        "Command failed: {:?}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);

    // Verify SIXEL sequence
    assert!(
        stdout.contains("\x1bPq") || stdout.starts_with("\x1bP"),
        "Missing SIXEL DCS start sequence"
    );
    assert!(
        stdout.contains("\x1b\\"),
        "Missing SIXEL sequence terminator"
    );
}

/// Test 4: WebP image with SIXEL protocol
#[test]
fn test_image_webp_sixel() {
    let mut cmd = Command::cargo_bin("emterm").unwrap();

    let output = cmd
        .arg("image")
        .arg("--protocol")
        .arg("sixel")
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

    // Verify SIXEL sequence
    assert!(
        stdout.contains("\x1bPq") || stdout.starts_with("\x1bP"),
        "Missing SIXEL DCS start sequence"
    );
    assert!(
        stdout.contains("\x1b\\"),
        "Missing SIXEL sequence terminator"
    );
}

/// Integration test: Invalid protocol option
#[test]
fn test_image_invalid_protocol() {
    let mut cmd = Command::cargo_bin("emterm").unwrap();

    let output = cmd
        .arg("image")
        .arg("--protocol")
        .arg("invalid_protocol")
        .arg("tests/fixtures/small.png")
        .output()
        .expect("Failed to execute command");

    // Should fail
    assert!(
        !output.status.success(),
        "Command should fail with invalid protocol"
    );

    let stderr = String::from_utf8_lossy(&output.stderr);
    // Error could be from clap (invalid value) or from application logic
    assert!(
        stderr.contains("invalid") || stderr.contains("protocol") || stderr.contains("value"),
        "Error message should mention invalid protocol: {}",
        stderr
    );
}

/// Integration test: Protocol case sensitivity
#[test]
fn test_image_protocol_case_sensitivity() {
    // Test uppercase SIXEL
    let mut cmd = Command::cargo_bin("emterm").unwrap();

    let output = cmd
        .arg("image")
        .arg("--protocol")
        .arg("SIXEL")
        .arg("tests/fixtures/small.png")
        .output()
        .expect("Failed to execute command");

    // Behavior depends on implementation - document what happens
    // Most likely should accept case-insensitive or reject with clear error
    if output.status.success() {
        // Case-insensitive accepted
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(
            stdout.contains("\x1bPq") || stdout.starts_with("\x1bP"),
            "Should produce SIXEL output if uppercase accepted"
        );
    } else {
        // Case-sensitive, should error
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            stderr.contains("invalid") || stderr.contains("protocol"),
            "Should provide clear error for case mismatch"
        );
    }
}
