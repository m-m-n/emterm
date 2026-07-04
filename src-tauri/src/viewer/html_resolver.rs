//! basedir-confined resource resolution for the HTML viewer's custom
//! scheme (FR5, NFR1).
//!
//! The HTML viewer serves `basedir`-relative local resource references
//! (images, CSS, JavaScript, fonts) through its custom URI scheme. This
//! module owns the *security-critical* pure logic — confining resolution
//! to `basedir`, rejecting path traversal, verifying the result survives a
//! post-canonicalization re-check (symlink defense), and enforcing a MIME
//! allowlist (SVG excluded, matching the existing image resolver's XSS
//! posture) — mirroring `viewer::image_resolver`. It is platform-
//! independent and fully unit-testable; `task0004`'s protocol handler
//! calls [`resolve_resource`] from the custom-scheme handler and registers
//! this module in `viewer/mod.rs`.

use std::path::{Component, Path, PathBuf};

/// Maximum size (bytes) of a basedir resource read fully into memory.
/// Mirrors `image_resolver::MAX_IMAGE_BYTES`; a resource larger than this
/// is refused to avoid OOM.
pub const MAX_RESOURCE_BYTES: u64 = 64 * 1024 * 1024;

/// Allowlisted extension → MIME type. The raster subset matches
/// `image_resolver::ALLOWED`; extended with text/CSS, JavaScript, and
/// common font formats for HTML-document needs. SVG is intentionally
/// absent (XSS).
const ALLOWED: &[(&str, &str)] = &[
    ("png", "image/png"),
    ("jpg", "image/jpeg"),
    ("jpeg", "image/jpeg"),
    ("gif", "image/gif"),
    ("webp", "image/webp"),
    ("bmp", "image/bmp"),
    ("ico", "image/x-icon"),
    ("css", "text/css"),
    ("js", "text/javascript"),
    ("woff", "font/woff"),
    ("woff2", "font/woff2"),
    ("ttf", "font/ttf"),
    ("otf", "font/otf"),
];

/// Why a resource request was refused (for logging / tests).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResolveError {
    /// No `basedir` was provided.
    NoBasedir,
    /// The requested path escaped `basedir` (traversal / absolute /
    /// symlink escape).
    OutsideBasedir,
    /// The extension is not on the allowlist (e.g. svg, extensionless).
    DisallowedMime,
    /// The file could not be read, or exceeded the size cap.
    Io,
}

/// Resolve a `basedir`-relative resource `request` path to an absolute
/// file path **strictly within** `basedir`, returning the resolved path
/// and its MIME type. Rejects:
/// - absolute request paths and `..` traversal that escapes `basedir`,
/// - extensions outside the allowlist (SVG excluded).
///
/// This does *not* touch the filesystem (so it's pure and testable); use
/// [`resolve_resource`] to also load the bytes with the symlink and size
/// checks applied.
pub fn resolve_resource_path(
    basedir: Option<&str>,
    request: &str,
) -> Result<(PathBuf, &'static str), ResolveError> {
    let basedir = basedir.ok_or(ResolveError::NoBasedir)?;
    let base = Path::new(basedir);

    // Reject absolute request paths outright — only basedir-relative
    // references are served.
    let req = Path::new(request);
    if req.is_absolute() {
        return Err(ResolveError::OutsideBasedir);
    }

    // Lexically normalize `base + request`, refusing any `..` that would
    // climb above the basedir prefix. We don't touch the filesystem for
    // the confinement check so symlink races can't widen the result.
    let mut normalized: Vec<Component> = base.components().collect();
    let base_len = normalized.len();
    for comp in req.components() {
        match comp {
            Component::CurDir => {}
            Component::ParentDir => {
                // Only allow popping components we added below base_len.
                if normalized.len() <= base_len {
                    return Err(ResolveError::OutsideBasedir);
                }
                normalized.pop();
            }
            Component::Normal(part) => normalized.push(Component::Normal(part)),
            // RootDir / Prefix in a relative path shouldn't occur, but if
            // they do they are an escape attempt.
            Component::RootDir | Component::Prefix(_) => {
                return Err(ResolveError::OutsideBasedir);
            }
        }
    }

    let resolved: PathBuf = normalized.iter().collect();
    // Defense in depth: the resolved path must still start with base.
    if !resolved.starts_with(base) {
        return Err(ResolveError::OutsideBasedir);
    }

    let mime = mime_for(&resolved).ok_or(ResolveError::DisallowedMime)?;
    Ok((resolved, mime))
}

/// Resolve and load the resource bytes for a `basedir`-relative `request`,
/// applying the default [`MAX_RESOURCE_BYTES`] size cap.
pub fn resolve_resource(
    basedir: Option<&str>,
    request: &str,
) -> Result<(Vec<u8>, &'static str), ResolveError> {
    resolve_resource_with_cap(basedir, request, MAX_RESOURCE_BYTES)
}

/// Same as [`resolve_resource`] but with an explicit size cap, so callers
/// (and tests) can exercise the boundary without allocating a
/// `MAX_RESOURCE_BYTES`-sized file.
pub fn resolve_resource_with_cap(
    basedir: Option<&str>,
    request: &str,
    max_bytes: u64,
) -> Result<(Vec<u8>, &'static str), ResolveError> {
    let (path, mime) = resolve_resource_path(basedir, request)?;

    // Symlink defense: re-assert confinement after canonicalization so that
    // a symlink inside basedir pointing outside (e.g. basedir/leak.png ->
    // /etc/hostname) cannot bypass the lexical check above.
    let canon_base =
        std::fs::canonicalize(basedir.unwrap_or("")).map_err(|_| ResolveError::OutsideBasedir)?;
    let canon_path = std::fs::canonicalize(&path).map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            ResolveError::Io
        } else {
            ResolveError::OutsideBasedir
        }
    })?;
    if !canon_path.starts_with(&canon_base) {
        return Err(ResolveError::OutsideBasedir);
    }

    // Cap the file size before reading the whole file into memory to avoid
    // OOM on an arbitrarily large basedir-referenced resource.
    let meta = std::fs::metadata(&canon_path).map_err(|_| ResolveError::Io)?;
    if meta.len() > max_bytes {
        log::warn!(
            "viewer: html resource {} exceeds {} byte cap ({} bytes), refusing",
            canon_path.display(),
            max_bytes,
            meta.len()
        );
        return Err(ResolveError::Io);
    }

    let bytes = std::fs::read(&canon_path).map_err(|_| ResolveError::Io)?;
    Ok((bytes, mime))
}

/// MIME type for an allowlisted extension, else `None` (SVG and any other
/// type are refused).
fn mime_for(path: &Path) -> Option<&'static str> {
    let ext = path.extension()?.to_str()?.to_ascii_lowercase();
    ALLOWED
        .iter()
        .find(|(e, _)| *e == ext)
        .map(|(_, mime)| *mime)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(label: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "emterm-htmlresolver-{label}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    // ── AC-1: allowed extensions resolve with correct MIME + bytes ──────

    #[test]
    fn resolves_each_allowed_extension_path() {
        for (ext, mime) in ALLOWED {
            let req = format!("asset.{ext}");
            let (_p, m) = resolve_resource_path(Some("/base"), &req).unwrap();
            assert_eq!(&m, mime, "ext {ext}");
        }
    }

    #[test]
    fn resolves_simple_relative_png() {
        let (path, mime) = resolve_resource_path(Some("/home/me/docs"), "img/a.png").unwrap();
        assert_eq!(path, PathBuf::from("/home/me/docs/img/a.png"));
        assert_eq!(mime, "image/png");
    }

    #[test]
    fn reads_bytes_and_mime_for_each_resource_category() {
        let dir = temp_dir("categories");
        let cases: &[(&str, &[u8], &str)] = &[
            ("pic.png", b"\x89PNG small", "image/png"),
            ("style.css", b"body{color:red}", "text/css"),
            ("script.js", b"console.log(1);", "text/javascript"),
            ("font.woff2", b"wOF2fontbytes", "font/woff2"),
        ];
        for (name, content, mime) in cases {
            std::fs::write(dir.join(name), content).unwrap();
            let (bytes, m) = resolve_resource(Some(dir.to_str().unwrap()), name).unwrap();
            assert_eq!(&bytes, content, "content for {name}");
            assert_eq!(&m, mime, "mime for {name}");
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    // ── AC-2: absolute paths denied ──────────────────────────────────────

    #[test]
    fn rejects_absolute_request() {
        let err = resolve_resource_path(Some("/home/me/docs"), "/etc/passwd.png").unwrap_err();
        assert_eq!(err, ResolveError::OutsideBasedir);
    }

    // ── AC-3: `..` traversal escaping vs staying inside basedir ─────────

    #[test]
    fn rejects_parent_traversal_escaping_basedir() {
        let err = resolve_resource_path(Some("/home/me/docs"), "../secret.png").unwrap_err();
        assert_eq!(err, ResolveError::OutsideBasedir);
    }

    #[test]
    fn rejects_deep_traversal_escape() {
        let err = resolve_resource_path(Some("/home/me/docs"), "a/../../../../etc/passwd.png")
            .unwrap_err();
        assert_eq!(err, ResolveError::OutsideBasedir);
    }

    #[test]
    fn allows_internal_dotdot_that_stays_within_basedir() {
        // docs/sub/../a.png == docs/a.png — still inside basedir, allowed.
        let (path, _) = resolve_resource_path(Some("/home/me/docs"), "sub/../a.png").unwrap();
        assert_eq!(path, PathBuf::from("/home/me/docs/a.png"));
    }

    // ── AC-4: symlink inside basedir pointing outside is denied ─────────

    #[test]
    #[cfg(unix)]
    fn rejects_symlink_escaping_basedir() {
        let dir = temp_dir("symlink");
        let link = dir.join("leak.png");
        // /etc/hostname exists and is readable on the CI/dev Linux hosts
        // this project targets (same target used by image_window.rs's
        // symlink-escape test).
        std::os::unix::fs::symlink("/etc/hostname", &link).unwrap();

        let err = resolve_resource(Some(dir.to_str().unwrap()), "leak.png").unwrap_err();
        assert_eq!(err, ResolveError::OutsideBasedir);

        let _ = std::fs::remove_dir_all(&dir);
    }

    // ── AC-5: size cap — over cap denied, exactly at cap served ─────────

    #[test]
    fn file_over_cap_is_denied_file_at_cap_is_served() {
        let dir = temp_dir("sizecap");
        let content = b"0123456789"; // 10 bytes
        let file = dir.join("a.css");
        std::fs::write(&file, content).unwrap();
        let basedir = dir.to_str().unwrap();

        // Cap smaller than the file size denies it.
        let err = resolve_resource_with_cap(Some(basedir), "a.css", 9).unwrap_err();
        assert_eq!(err, ResolveError::Io);

        // Cap exactly at the file size serves it.
        let (bytes, mime) = resolve_resource_with_cap(Some(basedir), "a.css", 10).unwrap();
        assert_eq!(bytes, content);
        assert_eq!(mime, "text/css");

        let _ = std::fs::remove_dir_all(&dir);
    }

    // ── AC-6: disallowed types (svg, extensionless) denied ──────────────

    #[test]
    fn rejects_svg_mime() {
        let err = resolve_resource_path(Some("/base"), "diagram.svg").unwrap_err();
        assert_eq!(err, ResolveError::DisallowedMime);
    }

    #[test]
    fn rejects_unknown_extension() {
        let err = resolve_resource_path(Some("/base"), "data.exe").unwrap_err();
        assert_eq!(err, ResolveError::DisallowedMime);
    }

    #[test]
    fn rejects_no_extension() {
        let err = resolve_resource_path(Some("/base"), "noext").unwrap_err();
        assert_eq!(err, ResolveError::DisallowedMime);
    }

    // ── misc boundary coverage ──────────────────────────────────────────

    #[test]
    fn rejects_when_no_basedir() {
        let err = resolve_resource_path(None, "a.png").unwrap_err();
        assert_eq!(err, ResolveError::NoBasedir);
    }

    #[test]
    fn extension_match_is_case_insensitive() {
        let (_p, mime) = resolve_resource_path(Some("/base"), "PHOTO.PNG").unwrap();
        assert_eq!(mime, "image/png");
    }
}
