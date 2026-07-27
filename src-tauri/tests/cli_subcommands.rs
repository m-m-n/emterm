//! Integration tests for the `emterm` CLI subcommands.
//!
//! These tests call `cli::run` in-process (via the library facade) and
//! inspect the returned exit code. Stdout side effects are not captured
//! here — the byte-format details are covered by the unit tests under
//! `cli::encoding::osc` and `cli::protocols::*`. The intent of this
//! file is to verify the dispatcher wires arguments correctly and that
//! error paths surface the expected exit codes.

use emterm::cli;
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
fn html_subcommand_with_fixture_returns_zero() {
    let path = "tests/fixtures/html/sample.html";
    let code = cli::run(&args(&["html", path]));
    assert_eq!(code, 0, "html subcommand should succeed on a real fixture");
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
fn html_subcommand_missing_file_returns_two() {
    let code = cli::run(&args(&["html", "/nonexistent/path/file.html"]));
    assert_eq!(code, 2, "missing file should map to exit code 2");
}

#[test]
fn markdown_subcommand_directory_path_returns_two() {
    let code = cli::run(&args(&["markdown", "/tmp"]));
    assert_eq!(code, 2, "directory path should map to exit code 2");
}

#[test]
fn html_subcommand_directory_path_returns_two() {
    let code = cli::run(&args(&["html", "/tmp"]));
    assert_eq!(code, 2, "directory path should map to exit code 2");
}

#[test]
fn html_subcommand_unsupported_extension_returns_one() {
    let mut tmp = NamedTempFile::with_suffix(".txt").unwrap();
    tmp.write_all(b"<html></html>").unwrap();
    tmp.flush().unwrap();

    let path = tmp.path().to_string_lossy().to_string();
    let code = cli::run(&args(&["html", &path]));
    assert_eq!(code, 1, "unsupported extension should map to exit code 1");
}

// --- agent-status subcommand ---
// References task0001 AC-6 (dispatcher plumbing + exit code) and AC-8
// (invalid state -> usage exit code). Byte-exact wire-format coverage
// lives in `emterm::agent_status`'s unit tests; tmux DCS-passthrough
// composition is covered in `emterm::cli::agent_status`'s unit tests.

#[test]
fn agent_status_subcommand_working_with_name_returns_zero() {
    let code = cli::run(&args(&["agent-status", "working", "--name", "claude"]));
    assert_eq!(code, 0, "agent-status working --name should succeed");
}

#[test]
fn agent_status_subcommand_all_states_return_zero() {
    for state in ["idle", "working", "blocked", "done"] {
        let code = cli::run(&args(&["agent-status", state]));
        assert_eq!(code, 0, "agent-status {state} should succeed");
    }
}

#[test]
fn agent_status_subcommand_clear_returns_zero() {
    let code = cli::run(&args(&["agent-status", "clear"]));
    assert_eq!(code, 0, "agent-status clear should succeed");
}

#[test]
fn agent_status_subcommand_invalid_state_returns_two() {
    let code = cli::run(&args(&["agent-status", "sleeping"]));
    assert_eq!(code, 2, "invalid state should map to the usage exit code");
}

#[test]
fn agent_status_subcommand_missing_state_returns_two() {
    let code = cli::run(&args(&["agent-status"]));
    assert_eq!(code, 2, "missing state argument should be a usage error");
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

// --- --version flag ---
// References task0001 AC-1..AC-3. The flag is handled in `main()` before
// `logging::init()`, so its behavior can only be observed by spawning the
// built binary (unlike the subcommands above, which go through the
// in-process `cli::run` facade). `CARGO_BIN_EXE_emterm` is the same
// technique `mux_throughput.rs` uses to spawn the built binary.

#[test]
fn version_flag_prints_crate_version_and_exits_zero() {
    let exe = env!("CARGO_BIN_EXE_emterm");
    let output = std::process::Command::new(exe)
        .arg("--version")
        .output()
        .expect("spawn emterm --version");

    assert!(output.status.success(), "--version should exit with status 0");
    let expected = format!("{}\n", env!("CARGO_PKG_VERSION"));
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        expected,
        "--version should print exactly the crate version followed by one newline"
    );
}

#[test]
fn version_flag_stderr_is_empty() {
    let exe = env!("CARGO_BIN_EXE_emterm");
    let output = std::process::Command::new(exe)
        .arg("--version")
        .output()
        .expect("spawn emterm --version");

    assert!(
        output.stderr.is_empty(),
        "--version should produce no stderr output"
    );
}

#[test]
fn version_flag_with_extra_args_behaves_identically() {
    let exe = env!("CARGO_BIN_EXE_emterm");
    let output = std::process::Command::new(exe)
        .args(["--version", "anything"])
        .output()
        .expect("spawn emterm --version anything");

    assert!(
        output.status.success(),
        "--version with trailing args should still exit with status 0"
    );
    let expected = format!("{}\n", env!("CARGO_PKG_VERSION"));
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        expected,
        "--version with trailing args should print the same version output as --version alone"
    );
    assert!(
        output.stderr.is_empty(),
        "--version with trailing args should produce no stderr output"
    );
}
