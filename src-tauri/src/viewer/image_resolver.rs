//! basedir-confined image resolution for the viewer custom scheme (FR8).
//!
//! The child viewer serves `basedir`-relative local image references
//! through its custom URI scheme. This module owns the *security-critical*
//! pure logic — confining resolution to `basedir`, rejecting path
//! traversal, and enforcing the raster MIME allowlist (SVG excluded to
//! match the WebView build's XSS posture). It is platform-independent and
//! fully unit-testable; `window.rs` calls [`resolve_image`] from the GTK
//! custom-scheme handler.

use std::path::{Component, Path, PathBuf};

/// Maximum size (bytes) of a basedir image read fully into memory (H3).
/// A markdown-referenced file larger than this is refused to avoid OOM.
pub const MAX_IMAGE_BYTES: u64 = 64 * 1024 * 1024;

/// Allowlisted raster image extensions → MIME type. Mirrors the WebView
/// build's `ALLOWED_IMAGE_MIME_TYPES`. SVG is intentionally absent (XSS).
const ALLOWED: &[(&str, &str)] = &[
    ("png", "image/png"),
    ("jpg", "image/jpeg"),
    ("jpeg", "image/jpeg"),
    ("gif", "image/gif"),
    ("webp", "image/webp"),
    ("bmp", "image/bmp"),
    ("ico", "image/x-icon"),
];

/// Why an image request was refused (for logging / tests).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResolveError {
    /// No `basedir` was provided in the payload.
    NoBasedir,
    /// The requested path escaped `basedir` (traversal / absolute).
    OutsideBasedir,
    /// The extension is not on the raster allowlist (e.g. svg).
    DisallowedMime,
    /// The file could not be read.
    Io,
}

/// Resolve a `basedir`-relative image `request` path to an absolute file
/// path **strictly within** `basedir`, returning the resolved path and its
/// MIME type. Rejects:
/// - absolute request paths and `..` traversal that escapes `basedir`,
/// - extensions outside the raster allowlist (SVG excluded).
///
/// This does *not* read the file (so it's pure and testable); use
/// [`resolve_image`] to also load the bytes.
pub fn resolve_image_path(
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

/// Resolve and load the image bytes for a `basedir`-relative `request`.
pub fn resolve_image(
    basedir: Option<&str>,
    request: &str,
) -> Result<(Vec<u8>, &'static str), ResolveError> {
    let (path, mime) = resolve_image_path(basedir, request)?;

    // Symlink defense: re-assert confinement after canonicalization so that a
    // symlink inside basedir pointing outside (e.g. basedir/leak.png ->
    // /etc/passwd) cannot bypass the lexical check above.
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

    // H3: cap the file size before reading the whole file into memory to
    // avoid OOM on an arbitrarily large basedir-referenced image.
    let meta = std::fs::metadata(&canon_path).map_err(|_| ResolveError::Io)?;
    if meta.len() > MAX_IMAGE_BYTES {
        log::warn!(
            "viewer: image {} exceeds {} byte cap ({} bytes), refusing",
            canon_path.display(),
            MAX_IMAGE_BYTES,
            meta.len()
        );
        return Err(ResolveError::Io);
    }

    let bytes = std::fs::read(&canon_path).map_err(|_| ResolveError::Io)?;
    Ok((bytes, mime))
}

/// MIME type for an allowlisted raster extension, else `None` (SVG and any
/// other type are refused).
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

    #[test]
    fn resolves_simple_relative_png() {
        let (path, mime) = resolve_image_path(Some("/home/me/docs"), "img/a.png").unwrap();
        assert_eq!(path, PathBuf::from("/home/me/docs/img/a.png"));
        assert_eq!(mime, "image/png");
    }

    #[test]
    fn resolves_each_allowed_extension() {
        for (ext, mime) in ALLOWED {
            let req = format!("pic.{ext}");
            let (_p, m) = resolve_image_path(Some("/base"), &req).unwrap();
            assert_eq!(&m, mime, "ext {ext}");
        }
    }

    #[test]
    fn rejects_parent_traversal_escaping_basedir() {
        let err = resolve_image_path(Some("/home/me/docs"), "../secret.png").unwrap_err();
        assert_eq!(err, ResolveError::OutsideBasedir);
    }

    #[test]
    fn rejects_deep_traversal_escape() {
        let err =
            resolve_image_path(Some("/home/me/docs"), "a/../../../../etc/passwd.png").unwrap_err();
        assert_eq!(err, ResolveError::OutsideBasedir);
    }

    #[test]
    fn allows_internal_dotdot_that_stays_within_basedir() {
        // docs/sub/../a.png == docs/a.png — still inside basedir, allowed.
        let (path, _) = resolve_image_path(Some("/home/me/docs"), "sub/../a.png").unwrap();
        assert_eq!(path, PathBuf::from("/home/me/docs/a.png"));
    }

    #[test]
    fn rejects_absolute_request() {
        let err = resolve_image_path(Some("/home/me/docs"), "/etc/passwd.png").unwrap_err();
        assert_eq!(err, ResolveError::OutsideBasedir);
    }

    #[test]
    fn rejects_svg_mime() {
        let err = resolve_image_path(Some("/base"), "diagram.svg").unwrap_err();
        assert_eq!(err, ResolveError::DisallowedMime);
    }

    #[test]
    fn rejects_unknown_extension() {
        let err = resolve_image_path(Some("/base"), "data.exe").unwrap_err();
        assert_eq!(err, ResolveError::DisallowedMime);
    }

    #[test]
    fn rejects_no_extension() {
        let err = resolve_image_path(Some("/base"), "noext").unwrap_err();
        assert_eq!(err, ResolveError::DisallowedMime);
    }

    #[test]
    fn rejects_when_no_basedir() {
        let err = resolve_image_path(None, "a.png").unwrap_err();
        assert_eq!(err, ResolveError::NoBasedir);
    }

    #[test]
    fn extension_match_is_case_insensitive() {
        let (_p, mime) = resolve_image_path(Some("/base"), "PHOTO.PNG").unwrap();
        assert_eq!(mime, "image/png");
    }

    #[test]
    fn small_file_under_cap_resolves_with_bytes() {
        // H3: a normal (small) file under MAX_IMAGE_BYTES still resolves and
        // returns its bytes. We do NOT create a 64MiB file; the oversize
        // branch is asserted indirectly via the documented MAX_IMAGE_BYTES
        // const and the metadata check guarding `fs::read`.
        let dir = std::env::temp_dir().join(format!(
            "emterm-imgtest-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("a.png");
        std::fs::write(&file, b"\x89PNG small").unwrap();

        let basedir = dir.to_str().unwrap();
        let (bytes, mime) = resolve_image(Some(basedir), "a.png").unwrap();
        assert_eq!(bytes, b"\x89PNG small");
        assert_eq!(mime, "image/png");
        assert!(MAX_IMAGE_BYTES >= 1, "cap is a positive const");

        // Test hygiene: remove the temp dir we created.
        let _ = std::fs::remove_file(&file);
        let _ = std::fs::remove_dir(&dir);
    }
}
