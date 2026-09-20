//! Asset-manifest consistency check (task0001, fetch-fonts-noto-color-emoji).
//!
//! Fails whenever a font embedded at compile time (an `include_bytes!`
//! literal whose path ends in `.otf` / `.ttf`) is not declared in both
//! `scripts/fetch-fonts.sh` and the inventory table in
//! `src-tauri/assets/fonts/README.md`. One-directional only (a declared but
//! no-longer-embedded font is not reported).
//!
//! Structured per IMPLEMENTATION.md decision D5: the comparison below is a
//! pure function over three sets of font file names and never touches the
//! filesystem. Only the repository-facing tests near the bottom read real
//! files, resolved from `CARGO_MANIFEST_DIR` (never the process's current
//! directory, never an absolute developer-machine path).

use std::collections::BTreeSet;
use std::path::{Component, Path, PathBuf};

/// Font extensions this project bundles (see `src-tauri/assets/fonts/`).
const FONT_EXTENSIONS: [&str; 2] = ["otf", "ttf"];

/// Build-output directories share this name prefix
/// (`target`, `target-host`, `target-win`, ...). Any directory whose name
/// starts with it is excluded from the walk (NFR5).
const BUILD_OUTPUT_PREFIX: &str = "target";

/// This file's own path, relative to the crate manifest directory. Excluded
/// from the repository walk so the synthetic `include_bytes!`-shaped
/// fixture text used by the tests below is never mistaken for a real
/// embed (TS5).
const SELF_RELATIVE_PATH: &str = "tests/asset_manifest.rs";

// ── One offending font (comparison output) ──────────────────────────

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct MissingDeclaration {
    font: String,
    missing_fetch_script: bool,
    missing_inventory: bool,
}

impl std::fmt::Display for MissingDeclaration {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut missing = Vec::new();
        if self.missing_fetch_script {
            missing.push("scripts/fetch-fonts.sh entry");
        }
        if self.missing_inventory {
            missing.push("assets/fonts/README.md inventory row");
        }
        write!(f, "{}: missing {}", self.font, missing.join(" and "))
    }
}

// ── Comparison responsibility (D5a): pure, no file access ───────────

/// Every embedded font absent from one or both declaration sets.
/// One-directional (D6): a declared-but-not-embedded font contributes
/// nothing.
fn find_missing_declarations(
    embedded: &BTreeSet<String>,
    fetch_declared: &BTreeSet<String>,
    inventory_declared: &BTreeSet<String>,
) -> Vec<MissingDeclaration> {
    embedded
        .iter()
        .filter_map(|font| {
            let missing_fetch_script = !fetch_declared.contains(font);
            let missing_inventory = !inventory_declared.contains(font);
            if missing_fetch_script || missing_inventory {
                Some(MissingDeclaration {
                    font: font.clone(),
                    missing_fetch_script,
                    missing_inventory,
                })
            } else {
                None
            }
        })
        .collect()
}

// ── Embedded-font collection (D5a): pure text parsing ────────────────

/// The file-name component of every `include_bytes!("...")` literal in
/// `text` whose path ends in one of `FONT_EXTENSIONS`.
fn embedded_font_names(text: &str) -> BTreeSet<String> {
    const NEEDLE: &str = "include_bytes!(";
    let mut out = BTreeSet::new();
    let mut rest = text;
    while let Some(start) = rest.find(NEEDLE) {
        let after_needle = &rest[start + NEEDLE.len()..];
        let Some(quote_start) = after_needle.find('"') else {
            break;
        };
        let after_open_quote = &after_needle[quote_start + 1..];
        let Some(quote_end) = after_open_quote.find('"') else {
            break;
        };
        let literal_path = &after_open_quote[..quote_end];
        if let Some(name) = Path::new(literal_path).file_name().and_then(|n| n.to_str()) {
            let is_font = FONT_EXTENSIONS.iter().any(|ext| {
                name.rsplit_once('.')
                    .map(|(_, actual_ext)| actual_ext.eq_ignore_ascii_case(ext))
                    .unwrap_or(false)
            });
            if is_font {
                out.insert(name.to_string());
            }
        }
        rest = &after_open_quote[quote_end + 1..];
    }
    out
}

// ── Fetch-script declaration set (D5a): pure text parsing ────────────

/// Font file names declared via `fetch_one "<name>" ...` in
/// `scripts/fetch-fonts.sh`'s text.
fn fetch_script_declared_names(text: &str) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    for line in text.lines() {
        let Some(rest) = line.trim_start().strip_prefix("fetch_one ") else {
            continue;
        };
        let Some(rest) = rest.trim_start().strip_prefix('"') else {
            continue;
        };
        if let Some(end) = rest.find('"') {
            out.insert(rest[..end].to_string());
        }
    }
    out
}

// ── Inventory declaration set (D5a): pure text parsing ────────────────

/// Font file names listed in the first column of the Markdown inventory
/// table (backtick-quoted names ending in a font extension).
fn inventory_declared_names(text: &str) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    for line in text.lines() {
        let trimmed = line.trim_start();
        if !trimmed.starts_with('|') {
            continue;
        }
        let Some(first_cell) = trimmed[1..].split('|').next() else {
            continue;
        };
        let cell = first_cell.trim();
        let Some(name) = cell.strip_prefix('`').and_then(|s| s.strip_suffix('`')) else {
            continue;
        };
        let is_font = FONT_EXTENSIONS
            .iter()
            .any(|ext| name.to_ascii_lowercase().ends_with(&format!(".{ext}")));
        if is_font {
            out.insert(name.to_string());
        }
    }
    out
}

// ── Scan-scope predicates (repository binding) ───────────────────────

/// `true` if `name` is a build-output directory that must never be walked
/// into (NFR5): `target`, `target-host`, `target-win`, or any sibling
/// sharing the same prefix.
fn is_build_output_dir(name: &str) -> bool {
    name.starts_with(BUILD_OUTPUT_PREFIX)
}

/// `true` if `relative_path` (relative to the crate manifest directory)
/// must be excluded from the walk: it sits under a build-output directory,
/// or it is this checking test's own source (TS5).
fn is_excluded_path(relative_path: &Path) -> bool {
    if relative_path == Path::new(SELF_RELATIVE_PATH) {
        return true;
    }
    relative_path.components().any(|component| match component {
        Component::Normal(name) => name.to_str().map(is_build_output_dir).unwrap_or(false),
        _ => false,
    })
}

// ── Repository binding: the only file-access code in this file ──────

fn crate_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// Recursively collect the text of every `.rs` file under `root`, paired
/// with its path relative to `root`. Skips build-output directories and
/// (when `root` is the crate root) this file's own source.
fn collect_rust_source(root: &Path, relative_prefix: &Path, out: &mut Vec<(PathBuf, String)>) {
    let Ok(entries) = std::fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(name_str) = name.to_str() else {
            continue;
        };
        let relative = relative_prefix.join(name_str);
        if is_excluded_path(&relative) {
            continue;
        }
        let path = entry.path();
        if path.is_dir() {
            if is_build_output_dir(name_str) {
                continue;
            }
            collect_rust_source(&path, &relative, out);
        } else if path.extension().and_then(|e| e.to_str()) == Some("rs") {
            if let Ok(text) = std::fs::read_to_string(&path) {
                out.push((relative, text));
            }
        }
    }
}

/// Every embedded font name found under `root`'s `.rs` files.
fn scan_embedded_fonts(root: &Path) -> BTreeSet<String> {
    let mut files = Vec::new();
    collect_rust_source(root, Path::new(""), &mut files);
    let mut out = BTreeSet::new();
    for (_, text) in &files {
        out.extend(embedded_font_names(text));
    }
    out
}

fn read_from_crate_root(relative: &str) -> String {
    let path = crate_root().join(relative);
    std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("asset_manifest: failed to read {}: {e}", path.display()))
}

// ── Test helpers ──────────────────────────────────────────────────────

fn names(values: &[&str]) -> BTreeSet<String> {
    values.iter().map(|s| s.to_string()).collect()
}

// ── AC-4 / TS2 / TS3: comparison responsibility, synthetic sets ─────

#[test]
fn font_missing_fetch_script_declaration_is_reported() {
    let embedded = names(&["Foo-Regular.ttf"]);
    let fetch_declared = names(&[]);
    let inventory_declared = names(&["Foo-Regular.ttf"]);
    let missing = find_missing_declarations(&embedded, &fetch_declared, &inventory_declared);
    assert_eq!(missing.len(), 1, "expected exactly one offending font");
    assert_eq!(missing[0].font, "Foo-Regular.ttf");
    assert!(missing[0].missing_fetch_script);
    assert!(!missing[0].missing_inventory);
    assert_eq!(
        missing[0].to_string(),
        "Foo-Regular.ttf: missing scripts/fetch-fonts.sh entry"
    );
}

#[test]
fn font_missing_inventory_row_is_reported() {
    let embedded = names(&["Foo-Regular.ttf"]);
    let fetch_declared = names(&["Foo-Regular.ttf"]);
    let inventory_declared = names(&[]);
    let missing = find_missing_declarations(&embedded, &fetch_declared, &inventory_declared);
    assert_eq!(missing.len(), 1, "expected exactly one offending font");
    assert!(!missing[0].missing_fetch_script);
    assert!(missing[0].missing_inventory);
    assert_eq!(
        missing[0].to_string(),
        "Foo-Regular.ttf: missing assets/fonts/README.md inventory row"
    );
}

#[test]
fn font_missing_both_declarations_is_reported() {
    let embedded = names(&["Foo-Regular.ttf"]);
    let fetch_declared = names(&[]);
    let inventory_declared = names(&[]);
    let missing = find_missing_declarations(&embedded, &fetch_declared, &inventory_declared);
    assert_eq!(missing.len(), 1, "expected exactly one offending font");
    assert!(missing[0].missing_fetch_script);
    assert!(missing[0].missing_inventory);
    assert_eq!(
        missing[0].to_string(),
        "Foo-Regular.ttf: missing scripts/fetch-fonts.sh entry and assets/fonts/README.md inventory row"
    );
}

#[test]
fn font_declared_in_both_is_not_reported() {
    let embedded = names(&["Foo-Regular.ttf"]);
    let fetch_declared = names(&["Foo-Regular.ttf"]);
    let inventory_declared = names(&["Foo-Regular.ttf"]);
    assert!(find_missing_declarations(&embedded, &fetch_declared, &inventory_declared).is_empty());
}

#[test]
fn declaration_without_matching_embed_is_not_reported() {
    // D6: the assertion is one-directional — a font declared in the fetch
    // script and/or inventory but no longer embedded contributes nothing.
    let embedded = names(&[]);
    let fetch_declared = names(&["Orphan.ttf"]);
    let inventory_declared = names(&["Orphan.ttf"]);
    assert!(find_missing_declarations(&embedded, &fetch_declared, &inventory_declared).is_empty());
}

// ── Parsing responsibilities: synthetic text ─────────────────────────

#[test]
fn fetch_script_names_are_parsed_from_fetch_one_calls() {
    let text = concat!(
        "set -euo pipefail\n",
        "\n",
        "fetch_one \"Alpha-Regular.ttf\" \\\n",
        "    \"https://example.invalid/a\" \\\n",
        "    \"deadbeef\"\n",
        "\n",
        "fetch_one \"Beta-Bold.otf\" \\\n",
        "    \"https://example.invalid/b\" \\\n",
        "    \"cafebabe\"\n",
    );
    assert_eq!(
        fetch_script_declared_names(text),
        names(&["Alpha-Regular.ttf", "Beta-Bold.otf"])
    );
}

#[test]
fn inventory_names_are_parsed_from_table_rows() {
    let text = concat!(
        "| File | Upstream | SHA-256 |\n",
        "|---|---|---|\n",
        "| `Alpha-Regular.ttf` | <https://example.invalid> | `deadbeef` |\n",
        "| `Beta-Bold.otf` | <https://example.invalid> | `cafebabe` |\n",
    );
    assert_eq!(
        inventory_declared_names(text),
        names(&["Alpha-Regular.ttf", "Beta-Bold.otf"])
    );
}

#[test]
fn embedded_font_names_extracts_basename_of_font_extension_literals() {
    let text = concat!(
        "pub const A: &[u8] =\n",
        "    include_bytes!(\"../../assets/fonts/Alpha-Regular.ttf\");\n",
        "pub const B: &[u8] = include_bytes!(\"../../assets/fonts/Beta-Bold.otf\");\n",
    );
    assert_eq!(
        embedded_font_names(text),
        names(&["Alpha-Regular.ttf", "Beta-Bold.otf"])
    );
}

// ── AC-6: non-font embeds are ignored ─────────────────────────────────

#[test]
fn non_font_extension_embed_is_ignored() {
    let text = r#"const LOGO: &[u8] = include_bytes!("../assets/icons/logo.png");"#;
    assert!(embedded_font_names(text).is_empty());
}

// ── AC-6 / TS6: build-output directories are excluded ─────────────────

#[test]
fn walk_excludes_build_output_directories() {
    for offending in [
        "target/debug/build/x/out.rs",
        "target-host/release/build/y/out.rs",
        "target-win/x86_64-pc-windows-msvc/release/build/z/out.rs",
        "target-anything-else/out.rs",
    ] {
        assert!(
            is_excluded_path(Path::new(offending)),
            "{offending} should be excluded as a build-output path"
        );
    }
    assert!(
        !is_excluded_path(Path::new("src/render/font/resolver.rs")),
        "an ordinary source path must not be excluded"
    );
}

// ── AC-6 / TS5: this test's own source is excluded ─────────────────────

#[test]
fn walk_excludes_its_own_source_file() {
    assert!(is_excluded_path(Path::new(SELF_RELATIVE_PATH)));
    assert!(!is_excluded_path(Path::new("tests/cli_subcommands.rs")));
}

// ── AC-5: no font binary ever needs to exist ──────────────────────────

#[test]
fn scan_requires_no_font_binaries_present() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    std::fs::create_dir_all(root.join("src")).expect("create src dir");
    // Deliberately no `assets/fonts/` directory at all under `root` — the
    // scan must still find the embed from source text alone.
    std::fs::write(
        root.join("src/fake.rs"),
        r#"pub const X: &[u8] = include_bytes!("../assets/fonts/Fake-Regular.ttf");"#,
    )
    .expect("write fake source");
    let embedded = scan_embedded_fonts(root);
    assert_eq!(embedded, names(&["Fake-Regular.ttf"]));
}

// ── AC-3 / AC-5: the repository-facing invariant ───────────────────────

/// The seven fonts embedded by the font resolver today (task0001 plan,
/// "Expected current state"). A drift here means either a font was added
/// without updating this list intentionally, or the scan scope broke.
fn documented_embedded_fonts() -> BTreeSet<String> {
    names(&[
        "NotoSansCJKjp-Regular.otf",
        "NotoSansCJKjp-Bold.otf",
        "Noto-COLRv1.ttf",
        "NotoEmoji-Regular.ttf",
        "Inconsolata-Regular.otf",
        "Inconsolata-Bold.otf",
        "NotoSansSymbols2-Regular.ttf",
    ])
}

#[test]
fn exactly_the_documented_fonts_are_embedded() {
    let embedded = scan_embedded_fonts(&crate_root());
    assert_eq!(
        embedded,
        documented_embedded_fonts(),
        "asset_manifest: the embedded-font set drifted from the documented inventory"
    );
}

#[test]
fn embedded_fonts_are_declared_in_fetch_script_and_inventory() {
    let embedded = scan_embedded_fonts(&crate_root());
    assert!(
        !embedded.is_empty(),
        "asset_manifest: no embedded fonts found at all — scan scope is likely broken"
    );

    let fetch_text = read_from_crate_root("../scripts/fetch-fonts.sh");
    let inventory_text = read_from_crate_root("assets/fonts/README.md");
    let fetch_declared = fetch_script_declared_names(&fetch_text);
    let inventory_declared = inventory_declared_names(&inventory_text);

    let missing = find_missing_declarations(&embedded, &fetch_declared, &inventory_declared);
    assert!(
        missing.is_empty(),
        "asset_manifest: font(s) embedded at compile time are missing a declaration:\n{}",
        missing
            .iter()
            .map(|m| m.to_string())
            .collect::<Vec<_>>()
            .join("\n")
    );
}
