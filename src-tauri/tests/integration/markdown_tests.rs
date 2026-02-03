use assert_cmd::Command;
use predicates::prelude::*;
use std::fs;
use std::io::Write;
use tempfile::NamedTempFile;

/// Test 1: Small Markdown file (< 1KB)
#[test]
fn test_markdown_small_file() {
    let mut cmd = Command::cargo_bin("emterm").unwrap();

    let output = cmd
        .arg("markdown")
        .arg("tests/fixtures/sample.md")
        .output()
        .expect("Failed to execute command");

    // Should succeed
    assert!(
        output.status.success(),
        "Command failed: {:?}",
        String::from_utf8_lossy(&output.stderr)
    );

    // Should output to stdout
    assert!(!output.stdout.is_empty(), "No output to stdout");

    let stdout = String::from_utf8_lossy(&output.stdout);

    // Verify OSC sequence structure
    assert!(
        stdout.contains("\x1b]777;emterm;markdown;begin"),
        "Missing OSC begin sequence"
    );
    assert!(
        stdout.contains("\x1b]777;emterm;markdown;end"),
        "Missing OSC end sequence"
    );
    assert!(stdout.contains("format=gfm"), "Missing format parameter");
    assert!(
        stdout.contains("render=fullscreen"),
        "Missing render parameter"
    );
    assert!(stdout.contains("version=1.0"), "Missing version parameter");
}

/// Test 2: Medium Markdown file (100KB)
#[test]
fn test_markdown_medium_file() {
    // Create a 100KB markdown file
    let mut temp_file = NamedTempFile::new().unwrap();
    let content = "# Test Heading\n\nThis is a test paragraph.\n\n".repeat(2000); // ~100KB
    write!(temp_file, "{}", content).unwrap();
    temp_file.flush().unwrap();

    let mut cmd = Command::cargo_bin("emterm").unwrap();

    let output = cmd
        .arg("markdown")
        .arg(temp_file.path())
        .output()
        .expect("Failed to execute command");

    // Should succeed
    assert!(
        output.status.success(),
        "Command failed: {:?}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);

    // Verify chunking occurs (should have multiple chunk sequences)
    let chunk_count = stdout.matches("\x1b]777;emterm;markdown;chunk").count();
    assert!(
        chunk_count > 1,
        "Expected multiple chunks for 100KB file, got {}",
        chunk_count
    );

    // Verify sequential seq numbers
    assert!(stdout.contains("seq=0"), "Missing seq=0");
    assert!(stdout.contains("seq=1"), "Missing seq=1");
}

/// Test 3: File at size limit (exactly 2MB)
#[test]
fn test_markdown_at_size_limit() {
    let mut cmd = Command::cargo_bin("emterm").unwrap();

    let output = cmd
        .arg("markdown")
        .arg("tests/fixtures/large.md")
        .output()
        .expect("Failed to execute command");

    // Verify file is close to 2MB
    let file_size = fs::metadata("tests/fixtures/large.md").unwrap().len();
    assert!(
        file_size >= 1_900_000 && file_size <= 2_097_152,
        "File size should be close to 2MB: {} bytes",
        file_size
    );

    // Should succeed
    assert!(
        output.status.success(),
        "Command should succeed for file at size limit: {:?}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("\x1b]777;emterm;markdown;begin"),
        "Missing OSC begin sequence"
    );
    assert!(
        stdout.contains("\x1b]777;emterm;markdown;end"),
        "Missing OSC end sequence"
    );
}

/// Test 4: File over size limit (2MB + 1 byte)
#[test]
fn test_markdown_over_size_limit() {
    // Create a file just over 2MB
    let mut temp_file = NamedTempFile::new().unwrap();
    let size = 2_097_153; // 2MB + 1 byte
    let content = vec![b'#'; size];
    temp_file.write_all(&content).unwrap();
    temp_file.flush().unwrap();

    let mut cmd = Command::cargo_bin("emterm").unwrap();

    let output = cmd
        .arg("markdown")
        .arg(temp_file.path())
        .output()
        .expect("Failed to execute command");

    // Should fail with exit code 1
    assert!(
        !output.status.success(),
        "Command should fail for oversized file"
    );
    assert_eq!(output.status.code(), Some(1), "Expected exit code 1");

    // Should output error to stderr
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

/// Test 5: Non-existent file
#[test]
fn test_markdown_nonexistent_file() {
    let mut cmd = Command::cargo_bin("emterm").unwrap();

    let output = cmd
        .arg("markdown")
        .arg("nonexistent_file_12345.md")
        .output()
        .expect("Failed to execute command");

    // Should fail with exit code 2 (I/O error)
    assert!(
        !output.status.success(),
        "Command should fail for non-existent file"
    );
    assert_eq!(
        output.status.code(),
        Some(2),
        "Expected exit code 2 for I/O error"
    );

    // Should output error to stderr
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("not found")
            || stderr.contains("No such file")
            || stderr.contains("\u{898b}\u{3064}\u{304b}\u{308a}\u{307e}\u{305b}\u{3093}"), // 見つかりません (ja)
        "Error message should mention file not found: {}",
        stderr
    );
}

/// Test 6: Empty file (0 bytes)
#[test]
fn test_markdown_empty_file() {
    // Create an empty file
    let temp_file = NamedTempFile::new().unwrap();
    // Don't write anything - file is empty

    let mut cmd = Command::cargo_bin("emterm").unwrap();

    let output = cmd
        .arg("markdown")
        .arg(temp_file.path())
        .output()
        .expect("Failed to execute command");

    // Should succeed (empty file is valid)
    assert!(
        output.status.success(),
        "Command should succeed for empty file: {:?}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);

    // Should have minimal valid sequence
    assert!(
        stdout.contains("\x1b]777;emterm;markdown;begin"),
        "Missing OSC begin sequence"
    );
    assert!(
        stdout.contains("\x1b]777;emterm;markdown;end"),
        "Missing OSC end sequence"
    );
}

/// Integration test: Verify base64 encoding roundtrip
#[test]
fn test_markdown_base64_roundtrip() {
    let test_content = "# Hello World\n\nThis is a **test** with *formatting*.";
    let mut temp_file = NamedTempFile::new().unwrap();
    write!(temp_file, "{}", test_content).unwrap();
    temp_file.flush().unwrap();

    let mut cmd = Command::cargo_bin("emterm").unwrap();

    let output = cmd
        .arg("markdown")
        .arg(temp_file.path())
        .output()
        .expect("Failed to execute command");

    assert!(output.status.success());

    let stdout = String::from_utf8_lossy(&output.stdout);

    // Extract base64 data from chunk sequences
    // Format: \x1b]777;emterm;markdown;chunk;id=UUID;seq=N;data=BASE64\x1b\
    let mut base64_data = String::new();
    for line in stdout.lines() {
        if let Some(data_start) = line.find("data=") {
            let data_end = line[data_start..]
                .find('\x1b')
                .unwrap_or(line.len() - data_start);
            let data = &line[data_start + 5..data_start + data_end];
            base64_data.push_str(data);
        }
    }

    // Decode and verify
    if !base64_data.is_empty() {
        use base64::{Engine as _, engine::general_purpose};
        let decoded = general_purpose::STANDARD
            .decode(&base64_data)
            .expect("Invalid base64");
        let decoded_str = String::from_utf8(decoded).expect("Invalid UTF-8");
        assert_eq!(
            decoded_str, test_content,
            "Decoded content should match original"
        );
    }
}
