use assert_cmd::Command;
use std::io::Write;
use tempfile::NamedTempFile;

/// Test 1: Small file download
#[test]
fn test_download_small_file() {
    let mut temp_file = NamedTempFile::new().unwrap();
    write!(temp_file, "Hello World").unwrap();
    temp_file.flush().unwrap();

    let mut cmd = Command::cargo_bin("emterm").unwrap();
    let output = cmd
        .arg("download")
        .arg(temp_file.path())
        .output()
        .expect("Failed to execute command");

    assert!(
        output.status.success(),
        "Command failed: {:?}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("\x1b]777;emterm;download;begin"));
    assert!(stdout.contains("\x1b]777;emterm;download;end"));
    assert!(stdout.contains("version=1.0"));
    assert!(stdout.contains("size=11")); // "Hello World" = 11 bytes
}

/// Test 2: Verify filename in begin sequence
#[test]
fn test_download_filename_in_output() {
    let mut temp_file = NamedTempFile::with_suffix(".txt").unwrap();
    write!(temp_file, "data").unwrap();
    temp_file.flush().unwrap();

    let mut cmd = Command::cargo_bin("emterm").unwrap();
    let output = cmd
        .arg("download")
        .arg(temp_file.path())
        .output()
        .expect("Failed to execute command");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);

    // Filename should be basename only (without directory path)
    assert!(stdout.contains("name="));
    // Should not contain directory separators in name
    assert!(!stdout.contains("name=/"));
}

/// Test 3: Non-existent file
#[test]
fn test_download_nonexistent_file() {
    let mut cmd = Command::cargo_bin("emterm").unwrap();
    let output = cmd
        .arg("download")
        .arg("nonexistent_file_12345.bin")
        .output()
        .expect("Failed to execute command");

    assert!(!output.status.success());
    assert_eq!(output.status.code(), Some(2));
}

/// Test 4: Directory as argument
#[test]
fn test_download_directory() {
    let mut cmd = Command::cargo_bin("emterm").unwrap();
    let output = cmd
        .arg("download")
        .arg("/tmp")
        .output()
        .expect("Failed to execute command");

    assert!(!output.status.success());
}

/// Test 5: Empty file
#[test]
fn test_download_empty_file() {
    let temp_file = NamedTempFile::new().unwrap();

    let mut cmd = Command::cargo_bin("emterm").unwrap();
    let output = cmd
        .arg("download")
        .arg(temp_file.path())
        .output()
        .expect("Failed to execute command");

    assert!(output.status.success());

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("size=0"));
    assert!(stdout.contains("\x1b]777;emterm;download;begin"));
    assert!(stdout.contains("\x1b]777;emterm;download;end"));
    // No chunks for empty file
    assert!(!stdout.contains("\x1b]777;emterm;download;chunk"));
}

/// Test 6: Missing --name with stdin (no file arg, no --name)
#[test]
fn test_download_stdin_without_name() {
    let mut cmd = Command::cargo_bin("emterm").unwrap();
    let output = cmd
        .arg("download")
        .write_stdin("some data")
        .output()
        .expect("Failed to execute command");

    assert!(!output.status.success());
    assert_eq!(output.status.code(), Some(2));
}

/// Test 7: Stdin with --name
#[test]
fn test_download_stdin_with_name() {
    let mut cmd = Command::cargo_bin("emterm").unwrap();
    let output = cmd
        .arg("download")
        .arg("--name")
        .arg("output.bin")
        .write_stdin("Hello from stdin")
        .output()
        .expect("Failed to execute command");

    assert!(
        output.status.success(),
        "Command failed: {:?}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("name=output.bin"));
    assert!(stdout.contains("size=16")); // "Hello from stdin" = 16 bytes
}

/// Test 8: Base64 roundtrip verification
#[test]
fn test_download_base64_roundtrip() {
    // Create a file with known binary content
    let test_data: Vec<u8> = (0..256).map(|i| i as u8).collect();
    let mut temp_file = NamedTempFile::new().unwrap();
    temp_file.write_all(&test_data).unwrap();
    temp_file.flush().unwrap();

    let mut cmd = Command::cargo_bin("emterm").unwrap();
    let output = cmd
        .arg("download")
        .arg(temp_file.path())
        .output()
        .expect("Failed to execute command");

    assert!(output.status.success());

    let stdout = String::from_utf8_lossy(&output.stdout);

    // Extract base64 data from chunk sequences
    let mut base64_data = String::new();
    for part in stdout.split("\x1b]777;emterm;download;chunk;") {
        if let Some(data_start) = part.find("data=") {
            let data_part = &part[data_start + 5..];
            if let Some(end) = data_part.find('\x1b') {
                base64_data.push_str(&data_part[..end]);
            }
        }
    }

    if !base64_data.is_empty() {
        use base64::{Engine as _, engine::general_purpose};
        let decoded = general_purpose::STANDARD
            .decode(&base64_data)
            .expect("Invalid base64");
        assert_eq!(decoded, test_data, "Decoded content should match original");
    }
}

/// Test 9: Large file produces multiple chunks
#[test]
fn test_download_large_file_chunking() {
    let mut temp_file = NamedTempFile::new().unwrap();
    // Create ~200KB file → base64 ~267KB → multiple 128KB chunks
    let content = vec![b'A'; 200 * 1024];
    temp_file.write_all(&content).unwrap();
    temp_file.flush().unwrap();

    let mut cmd = Command::cargo_bin("emterm").unwrap();
    let output = cmd
        .arg("download")
        .arg(temp_file.path())
        .output()
        .expect("Failed to execute command");

    assert!(output.status.success());

    let stdout = String::from_utf8_lossy(&output.stdout);
    let chunk_count = stdout.matches("\x1b]777;emterm;download;chunk").count();
    assert!(
        chunk_count > 1,
        "Expected multiple chunks for 200KB file, got {}",
        chunk_count
    );
    assert!(stdout.contains("seq=0"));
    assert!(stdout.contains("seq=1"));
}
