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
use std::process::Command;

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

/// The seven relative paths bundled fonts are `include_bytes!`-ed from
/// (FR1). This exact set is fixed and must not be edited by any task.
const REQUIRED_FONTS: [&str; 7] = [
    "assets/fonts/Noto-COLRv1.ttf",
    "assets/fonts/NotoSansCJKjp-Regular.otf",
    "assets/fonts/NotoSansCJKjp-Bold.otf",
    "assets/fonts/NotoEmoji-Regular.ttf",
    "assets/fonts/Inconsolata-Regular.otf",
    "assets/fonts/Inconsolata-Bold.otf",
    "assets/fonts/NotoSansSymbols2-Regular.ttf",
];

/// Opt-out switch (FR3): `1` forbids the automatic fetch outright, on every
/// host. Unset, empty, or any other value: not engaged (D3).
const SKIP_FETCH_ENV: &str = "EMTERM_SKIP_FONT_FETCH";

/// GUI-feature bootstrap gate (FR2/FR3/FR4/D1-D5/D8): every font referenced
/// by `include_bytes!` must exist on disk before `cargo build` is allowed to
/// proceed. On finding any missing, this attempts to fetch the whole set
/// exactly once through `scripts/fetch-fonts.sh` (the project's single
/// acquisition path, used unchanged — FR5), unless the opt-out is engaged.
/// Every stop path ends with the same actionable message, preceded by
/// diagnostic context naming which of D8's four failure shapes occurred.
fn check_bundled_fonts() {
    let manifest_dir =
        PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR is set by cargo"));
    // D1: destination and script location are derived from the manifest
    // location, never from the build process's current directory, so a
    // build driven from an arbitrary cwd still targets this worktree.
    let worktree_root = manifest_dir
        .parent()
        .expect("CARGO_MANIFEST_DIR (src-tauri) has a parent (the worktree root)")
        .to_path_buf();

    // D5: declare the rerun inputs on every execution that reaches this
    // check, before any existence check runs, so the declared set never
    // depends on how far the check gets.
    for rel in REQUIRED_FONTS {
        println!("cargo:rerun-if-changed={rel}");
    }
    println!("cargo:rerun-if-env-changed={SKIP_FETCH_ENV}");

    let missing = missing_fonts(&manifest_dir);
    if missing.is_empty() {
        // Steady state (NFR2): all seven present, no invocation, no
        // environment inspection beyond the rerun declarations above, no
        // network access.
        return;
    }

    // D3: the opt-out forbids, it does not merely tolerate — stop before
    // anything that could reach the network is prepared.
    let skip_fetch = env::var(SKIP_FETCH_ENV).as_deref() == Ok("1");
    if skip_fetch {
        stop(
            &format!(
                "build_rs.font_fetch_skipped: {SKIP_FETCH_ENV}=1; automatic font fetch is disabled"
            ),
            missing[0],
            None,
        );
    }

    // D2/D4: attempt the acquisition exactly once, on every host, with no
    // interpreter-resolvability preflight.
    let script = worktree_root.join("scripts").join("fetch-fonts.sh");
    let dest_dir = manifest_dir.join("assets").join("fonts");
    // Passed in a representation the host's bash accepts (D1): forward
    // slashes even on Windows, matching this file's existing convention for
    // paths handed to external tools (see `emit_bundle_manifest`).
    let dest_dir_arg = dest_dir.to_string_lossy().replace('\\', "/");
    let script_arg = script.to_string_lossy().replace('\\', "/");

    match Command::new("bash")
        .arg(&script_arg)
        .current_dir(&worktree_root)
        .env("DEST_DIR", &dest_dir_arg)
        .output()
    {
        // D8 shape 1: the interpreter itself could not be launched. Re-
        // running the actionable message's command cannot fix this, so it
        // gets one further line naming the real remedy.
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => stop(
            &format!(
                "build_rs.font_fetch_launch_failed: could not launch the bash interpreter via PATH: {e}"
            ),
            missing[0],
            Some(
                "build_rs.font_fetch_launch_failed: bash must be made available on PATH before \
                 this can succeed; re-running the suggested command will not resolve this.",
            ),
        ),
        // D8 shape 2: some other launch error (permission denied and
        // similar).
        Err(e) => stop(
            &format!(
                "build_rs.font_fetch_launch_failed: could not launch scripts/fetch-fonts.sh via bash: {e}"
            ),
            missing[0],
            None,
        ),
        // D8 shape 3: the script ran and exited non-zero. Its own stderr is
        // preserved, not swallowed.
        Ok(output) if !output.status.success() => {
            let stderr = String::from_utf8_lossy(&output.stderr);
            stop(
                &format!(
                    "build_rs.font_fetch_failed: scripts/fetch-fonts.sh exited with {}\n{stderr}",
                    output.status
                ),
                missing[0],
                None,
            );
        }
        Ok(_) => {}
    }

    // D8 shape 4: the script exited zero but at least one font is still
    // absent.
    if let Some(&path) = missing_fonts(&manifest_dir).first() {
        stop(
            "build_rs.font_still_missing: scripts/fetch-fonts.sh exited 0 but a bundled font is \
             still absent",
            path,
            None,
        );
    }
}

/// Returns the entries of [`REQUIRED_FONTS`] not present on disk, checked as
/// absolute paths derived from `CARGO_MANIFEST_DIR` (D1) so the result never
/// depends on the build process's current directory.
fn missing_fonts(manifest_dir: &Path) -> Vec<&'static str> {
    REQUIRED_FONTS
        .iter()
        .copied()
        .filter(|rel| !manifest_dir.join(rel).exists())
        .collect()
}

/// Ends the build: `context` is the D8 failure-shape (or opt-out)
/// diagnostic, then the actionable message's two lines — unchanged in
/// wording and order, naming `path` — are emitted verbatim, then `extra`
/// (shape 1 only) if present.
fn stop(context: &str, path: &str, extra: Option<&str>) -> ! {
    let mut msg = format!(
        "{context}\n\
         build_rs.font_missing: bundled font missing at {path}\n  \
         Run `make fetch-fonts` (or `bash scripts/fetch-fonts.sh`) to download bundled fonts."
    );
    if let Some(extra) = extra {
        msg.push('\n');
        msg.push_str(extra);
    }
    panic!("{msg}");
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
