//! Integration tests for the `emterm-native-poc` CLI subcommands.
//!
//! These tests call `cli::run` in-process (via the library facade) and
//! inspect the returned exit code. Stdout side effects are not captured
//! here — the byte-format details are covered by the unit tests under
//! `cli::encoding::osc` and `cli::protocols::*`. The intent of this
//! file is to verify the dispatcher wires arguments correctly and that
//! error paths surface the expected exit codes.

use emterm_native_poc::cli;
use image::{DynamicImage, ImageFormat, RgbaImage};
use std::io::Write;
use tempfile::NamedTempFile;

fn args(parts: &[&str]) -> Vec<String> {
    parts.iter().map(|s| s.to_string()).collect()
}

#[test]
fn markdown_subcommand_with_fixture_returns_zero() {
    let path = "tests/fixtures/markdown/sample.md";
    let code = cli::run(&args(&["markdown", path]));
    assert_eq!(
        code, 0,
        "markdown subcommand should succeed on a real fixture"
    );
}

#[test]
fn json_subcommand_with_fixture_returns_zero() {
    let path = "tests/fixtures/data/sample.json";
    let code = cli::run(&args(&["json", path]));
    assert_eq!(code, 0, "json subcommand should succeed on a real fixture");
}

#[test]
fn yaml_subcommand_with_fixture_returns_zero() {
    let path = "tests/fixtures/data/sample.yaml";
    let code = cli::run(&args(&["yaml", path]));
    assert_eq!(code, 0, "yaml subcommand should succeed on a real fixture");
}

#[test]
fn image_subcommand_default_kitty_returns_zero() {
    // Create a small valid PNG in a temp file (we cannot ship one
    // as a binary asset without committing it; tempfile keeps the test
    // self-contained).
    let mut tmp = NamedTempFile::with_suffix(".png").unwrap();
    let img = DynamicImage::ImageRgba8(RgbaImage::new(4, 4));
    img.write_to(&mut tmp, ImageFormat::Png).unwrap();
    tmp.flush().unwrap();

    let path = tmp.path().to_string_lossy().to_string();
    let code = cli::run(&args(&["image", &path]));
    assert_eq!(
        code, 0,
        "image subcommand should default to kitty and succeed"
    );
}

#[test]
fn image_subcommand_sixel_returns_zero() {
    let mut tmp = NamedTempFile::with_suffix(".png").unwrap();
    let img = DynamicImage::ImageRgba8(RgbaImage::new(4, 4));
    img.write_to(&mut tmp, ImageFormat::Png).unwrap();
    tmp.flush().unwrap();

    let path = tmp.path().to_string_lossy().to_string();
    let code = cli::run(&args(&["image", &path, "--protocol", "sixel"]));
    assert_eq!(
        code, 0,
        "image subcommand with --protocol sixel should succeed"
    );
}

#[test]
fn image_subcommand_invalid_protocol_returns_one() {
    let mut tmp = NamedTempFile::with_suffix(".png").unwrap();
    let img = DynamicImage::ImageRgba8(RgbaImage::new(4, 4));
    img.write_to(&mut tmp, ImageFormat::Png).unwrap();
    tmp.flush().unwrap();

    let path = tmp.path().to_string_lossy().to_string();
    let code = cli::run(&args(&["image", &path, "--protocol", "ascii"]));
    assert_eq!(code, 1, "invalid --protocol should map to exit code 1");
}

#[test]
fn markdown_subcommand_missing_file_returns_two() {
    let code = cli::run(&args(&["markdown", "/nonexistent/path/file.md"]));
    assert_eq!(code, 2, "missing file should map to exit code 2");
}

#[test]
fn json_subcommand_missing_file_returns_two() {
    let code = cli::run(&args(&["json", "/nonexistent/path/file.json"]));
    assert_eq!(code, 2, "missing file should map to exit code 2");
}

#[test]
fn yaml_subcommand_missing_file_returns_two() {
    let code = cli::run(&args(&["yaml", "/nonexistent/path/file.yaml"]));
    assert_eq!(code, 2, "missing file should map to exit code 2");
}

#[test]
fn image_subcommand_missing_file_returns_two() {
    let code = cli::run(&args(&["image", "/nonexistent/path/image.png"]));
    assert_eq!(code, 2, "missing file should map to exit code 2");
}

#[test]
fn markdown_subcommand_directory_path_returns_two() {
    let code = cli::run(&args(&["markdown", "/tmp"]));
    assert_eq!(code, 2, "directory path should map to exit code 2");
}

#[test]
fn image_subcommand_unsupported_format_returns_one() {
    // A plain text file is not a recognized image format; magic-byte
    // validation should reject it.
    let mut tmp = NamedTempFile::with_suffix(".txt").unwrap();
    tmp.write_all(b"this is not an image at all").unwrap();
    tmp.flush().unwrap();

    let path = tmp.path().to_string_lossy().to_string();
    let code = cli::run(&args(&["image", &path]));
    assert_eq!(
        code, 1,
        "unsupported image format should map to exit code 1"
    );
}
