//! Source-hygiene scan for `src-tauri/examples/` (task0001,
//! example-font-absolute-path).
//!
//! Fails whenever any Rust source under `src-tauri/examples/` contains a
//! developer-machine absolute path literal (`/home/`-prefixed) or a
//! leftover `native-poc/` reference from the pre-promotion build layout
//! (see `.claude/rules/core-build-location.md`). The two checks are
//! independent test cases (TS1 / TS2) so a break in one does not mask the
//! other.
//!
//! Pure text scanning only: no font binaries need to exist, no example is
//! executed, no network access happens (NFR1). Scan scope is
//! `src-tauri/examples/` alone; this file lives under `src-tauri/tests/`
//! and is therefore outside that scope without needing an explicit
//! self-exclusion (see IMPLEMENTATION.md "走査スコープ").

use std::path::{Path, PathBuf};

/// Marker for a developer-machine absolute path literal.
const ABSOLUTE_PATH_MARKER: &str = "/home/";

/// Marker for the pre-promotion `native-poc/` build layout.
const NATIVE_POC_MARKER: &str = "native-poc/";

// ── One offending line (scan output) ────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq)]
struct Violation {
    file: String,
    line: usize,
    text: String,
}

impl std::fmt::Display for Violation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}:{}: {}", self.file, self.line, self.text.trim())
    }
}

// ── Marker-scan responsibility: pure, no file access ─────────────────────

/// Every line in `text` containing `marker`, as a `Violation` tagged with
/// `file` and its 1-based line number.
fn find_marker_violations(file: &str, text: &str, marker: &str) -> Vec<Violation> {
    text.lines()
        .enumerate()
        .filter(|(_, line)| line.contains(marker))
        .map(|(idx, line)| Violation {
            file: file.to_string(),
            line: idx + 1,
            text: line.to_string(),
        })
        .collect()
}

/// Panics if `files` is empty. A scan that silently resolved to 0 files
/// must never be reported as "no violations found" (AC-5).
fn require_non_empty_scan(files: &[(String, String)], scanned_dir: &Path) {
    assert!(
        !files.is_empty(),
        "examples_source_hygiene: scan found 0 files under {} — scan scope is likely broken",
        scanned_dir.display()
    );
}

fn scan_for_marker(files: &[(String, String)], marker: &str) -> Vec<Violation> {
    files
        .iter()
        .flat_map(|(file, text)| find_marker_violations(file, text, marker))
        .collect()
}

// ── Repository binding: the only file-access code in this file ──────────

fn examples_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("examples")
}

/// Every `.rs` file directly under `src-tauri/examples/`, paired with its
/// text.
fn collect_example_sources(dir: &Path) -> Vec<(String, String)> {
    let entries = std::fs::read_dir(dir).unwrap_or_else(|e| {
        panic!(
            "examples_source_hygiene: failed to read {}: {e}",
            dir.display()
        )
    });
    let mut out = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("rs") {
            continue;
        }
        let name = entry.file_name().to_string_lossy().to_string();
        let text = std::fs::read_to_string(&path).unwrap_or_else(|e| {
            panic!(
                "examples_source_hygiene: failed to read {}: {e}",
                path.display()
            )
        });
        out.push((name, text));
    }
    out
}

// ── Parsing responsibility: synthetic text ────────────────────────────────

#[test]
fn marker_violation_reports_file_and_line() {
    let text = "fn main() {\n    let p = \"/home/user/font.otf\";\n}\n";
    let violations = find_marker_violations("sample.rs", text, ABSOLUTE_PATH_MARKER);
    assert_eq!(violations.len(), 1, "expected exactly one violation");
    assert_eq!(violations[0].file, "sample.rs");
    assert_eq!(violations[0].line, 2);
    assert_eq!(
        violations[0].to_string(),
        "sample.rs:2: let p = \"/home/user/font.otf\";"
    );
}

#[test]
fn text_without_marker_has_no_violations() {
    let text = "fn main() {\n    println!(\"hi\");\n}\n";
    assert!(find_marker_violations("sample.rs", text, ABSOLUTE_PATH_MARKER).is_empty());
    assert!(find_marker_violations("sample.rs", text, NATIVE_POC_MARKER).is_empty());
}

#[test]
fn native_poc_marker_is_detected_independently_of_absolute_path_marker() {
    let text = "//! Run: cargo run --manifest-path native-poc/Cargo.toml\n";
    assert!(find_marker_violations("sample.rs", text, ABSOLUTE_PATH_MARKER).is_empty());
    let violations = find_marker_violations("sample.rs", text, NATIVE_POC_MARKER);
    assert_eq!(violations.len(), 1);
    assert_eq!(violations[0].line, 1);
}

// ── AC-5: empty scan scope is a failure condition, not a silent pass ─────

#[test]
#[should_panic(expected = "scan scope is likely broken")]
fn empty_scan_scope_panics() {
    require_non_empty_scan(&[], Path::new("examples"));
}

#[test]
fn non_empty_scan_scope_does_not_panic() {
    require_non_empty_scan(
        &[("a.rs".to_string(), String::new())],
        Path::new("examples"),
    );
}

// ── AC-1 / AC-2 / AC-4: no absolute developer-machine path literals (TS1) ─

#[test]
fn no_absolute_home_path_literals_in_examples() {
    let dir = examples_dir();
    let files = collect_example_sources(&dir);
    require_non_empty_scan(&files, &dir);
    let violations = scan_for_marker(&files, ABSOLUTE_PATH_MARKER);
    assert!(
        violations.is_empty(),
        "examples_source_hygiene: absolute path literal(s) found:\n{}",
        violations
            .iter()
            .map(|v| v.to_string())
            .collect::<Vec<_>>()
            .join("\n")
    );
}

// ── AC-3 / AC-4: no leftover native-poc/ execution instructions (TS2) ────

#[test]
fn no_native_poc_references_in_examples() {
    let dir = examples_dir();
    let files = collect_example_sources(&dir);
    require_non_empty_scan(&files, &dir);
    let violations = scan_for_marker(&files, NATIVE_POC_MARKER);
    assert!(
        violations.is_empty(),
        "examples_source_hygiene: native-poc/ reference(s) found:\n{}",
        violations
            .iter()
            .map(|v| v.to_string())
            .collect::<Vec<_>>()
            .join("\n")
    );
}
