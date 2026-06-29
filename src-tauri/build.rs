//! Build script for the `emterm` crate.
//!
//! Generates compile-time manifests of the embedded web bundles:
//! - the Markdown viewer (`viewer/dist/`, from `bun run build:viewer`)
//! - the settings window (`settings/dist/`, from `bun run build:settings`)
//!
//! The bundler emits content-hashed filenames, so we cannot
//! `include_bytes!` fixed paths; instead we walk each directory at build
//! time and emit a `&[ViewerAsset]` slice that `src/viewer/assets.rs` /
//! `src/settings_window/assets.rs` re-export.
//!
//! When the `gui` feature is disabled (CLI-only build via
//! `--no-default-features`) the GUI modules are not compiled at all, so
//! the manifests would be unused. We short-circuit in that case and skip
//! the dist directory check entirely — the CLI build must work without
//! Bun being installed.
//!
//! When `gui` is enabled but a dist directory is absent (e.g. a Rust-only
//! `cargo check` before the bundle is built), the manifest is empty and
//! the window serves a clear fallback. This keeps `cargo check` /
//! `cargo test` working without Bun.

use std::env;
use std::fs;
use std::path::{Path, PathBuf};

fn main() {
    // Windows resource embed runs unconditionally on Windows targets — it
    // covers both the GUI and the CLI-only build (FR1 / NFR1). Gating
    // sits on `CARGO_CFG_TARGET_OS == "windows"` so the crate, the linker
    // step, and the `rerun-if-changed` line are all skipped on Linux.
    if env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        embed_windows_icon_resource();
    }

    // CARGO_FEATURE_<NAME> is set by Cargo when the feature is enabled.
    // Skip the manifest emission for CLI-only builds.
    if env::var_os("CARGO_FEATURE_GUI").is_none() {
        return;
    }

    // GUI builds embed bundled fonts via include_bytes!. Fail fast here
    // when the files have not been fetched yet — the alternative is a
    // confusing include_bytes! error pointing into the asset directory.
    check_bundled_fonts();

    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());

    emit_bundle_manifest(
        &manifest_dir.join("viewer").join("dist"),
        "viewer/dist",
        "build:viewer",
        "Markdown viewer",
        "VIEWER_ASSETS",
        &out_dir.join("viewer_assets.rs"),
    );
    emit_bundle_manifest(
        &manifest_dir.join("settings").join("dist"),
        "settings/dist",
        "build:settings",
        "settings window",
        "SETTINGS_ASSETS",
        &out_dir.join("settings_assets.rs"),
    );
}

/// GUI-feature failsafe: every font referenced by `include_bytes!` must
/// exist on disk before `cargo build` is allowed to proceed. Without this
/// the developer would see an opaque `couldn't read assets/fonts/...`
/// error from `rustc`; emit an actionable message instead.
fn check_bundled_fonts() {
    let required = [
        "assets/fonts/NotoColorEmoji.ttf",
        "assets/fonts/NotoSansCJKjp-Regular.otf",
        "assets/fonts/NotoEmoji-Regular.ttf",
        "assets/fonts/Inconsolata-Regular.ttf",
        "assets/fonts/NotoSansSymbols2-Regular.ttf",
    ];
    for path in required {
        if !std::path::Path::new(path).exists() {
            panic!(
                "build_rs.font_missing: bundled font missing at {path}\n  \
                 Run `make fetch-fonts` (or `bash scripts/fetch-fonts.sh`) to download bundled fonts."
            );
        }
        println!("cargo:rerun-if-changed={path}");
    }
}

/// Windows-target-only: attach `icons/icon.ico` to the PE resource section
/// of `emterm.exe` so Explorer, the taskbar, and Alt+Tab render the eMterm
/// icon (FR1). Called only when `CARGO_CFG_TARGET_OS == "windows"`, so the
/// `winresource` crate is never invoked on Linux/macOS.
fn embed_windows_icon_resource() {
    // Cargo re-runs the build script when the icon changes. The path is
    // relative to `CARGO_MANIFEST_DIR` (i.e. the `src-tauri/` crate root).
    println!("cargo:rerun-if-changed=icons/icon.ico");
    if let Err(e) = winresource::WindowsResource::new()
        .set_icon("icons/icon.ico")
        .compile()
    {
        // Fail fast: a Windows build without the icon is a regression we
        // want to surface at build time, not at runtime.
        panic!("winresource: failed to embed icons/icon.ico: {e}");
    }
}

/// Walk `dist` and write a `pub static <slice_name>: &[ViewerAsset]`
/// manifest to `dest`. `rel_dist` is the manifest-dir-relative path used
/// for `rerun-if-changed`; `bun_script`/`label` shape the missing-bundle
/// diagnostics.
fn emit_bundle_manifest(
    dist: &Path,
    rel_dist: &str,
    bun_script: &str,
    label: &str,
    slice_name: &str,
    dest: &Path,
) {
    // Re-run if the bundle changes.
    println!("cargo:rerun-if-changed={rel_dist}");

    let mut entries: Vec<(String, PathBuf)> = Vec::new();
    if dist.is_dir() {
        collect(dist, dist, &mut entries);
    } else {
        let profile = env::var("PROFILE").unwrap_or_default();
        let msg = format!(
            "{rel_dist} is missing — run `bun run {bun_script}` first to embed the \
             {label} assets. Without this the {label} will always fail at runtime."
        );
        if profile == "release" {
            panic!("{}", msg);
        } else {
            println!("cargo:warning={}", msg);
        }
    }
    entries.sort_by(|a, b| a.0.cmp(&b.0));

    let mut code = String::new();
    code.push_str(&format!(
        "// @generated by build.rs — embedded {label} bundle manifest.\n\
         /// One embedded bundle file: (relative path, bytes, content type).\n\
         pub struct ViewerAsset {{\n\
         \x20   pub path: &'static str,\n\
         \x20   pub bytes: &'static [u8],\n\
         \x20   pub content_type: &'static str,\n\
         }}\n\n\
         /// All embedded {label} bundle files. Empty when `{rel_dist}` was\n\
         /// absent at build time (Rust-only build without `bun run {bun_script}`).\n\
         pub static {slice_name}: &[ViewerAsset] = &[\n"
    ));
    for (rel, abs) in &entries {
        let abs_str = abs.to_string_lossy().replace('\\', "/");
        let ct = content_type(rel);
        code.push_str(&format!(
            "    ViewerAsset {{ path: {rel:?}, bytes: include_bytes!({abs_str:?}), content_type: {ct:?} }},\n"
        ));
        println!("cargo:rerun-if-changed={rel_dist}/{rel}");
    }
    code.push_str("];\n");

    fs::write(dest, code).expect("write bundle manifest");
}

fn collect(root: &Path, dir: &Path, out: &mut Vec<(String, PathBuf)>) {
    let Ok(read) = fs::read_dir(dir) else {
        return;
    };
    for entry in read.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect(root, &path, out);
        } else if path.is_file() {
            if let Ok(rel) = path.strip_prefix(root) {
                let rel = rel.to_string_lossy().replace('\\', "/");
                out.push((rel, path));
            }
        }
    }
}

fn content_type(path: &str) -> &'static str {
    match path.rsplit('.').next().unwrap_or("") {
        "html" => "text/html; charset=utf-8",
        "js" | "mjs" => "text/javascript; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "json" | "map" => "application/json; charset=utf-8",
        "wasm" => "application/wasm",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "woff" => "font/woff",
        "woff2" => "font/woff2",
        "ttf" => "font/ttf",
        "otf" => "font/otf",
        _ => "application/octet-stream",
    }
}
