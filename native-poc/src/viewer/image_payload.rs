//! Image-viewer payload file format.
//!
//! The parent terminal writes one decoded image to a temp file and spawns
//! `self --image-viewer <path>`; the child reads it back on startup. The
//! format is a single-line JSON header (dimensions + chrome appearance)
//! followed by the raw un-encoded RGBA bytes, so a multi-megapixel image
//! never round-trips through base64/JSON:
//!
//! ```text
//! {"width":W,"height":H,"theme":"dark","preset":"pink","uiFontFamily":"..."}\n
//! <W*H*4 raw RGBA bytes>
//! ```
//!
//! The appearance tokens carry the PARENT's resolved settings into the
//! child (same design as the Markdown viewer's `PayloadAppearance`), so
//! the child never re-reads `settings.json` and cannot drift from the
//! parent's in-memory state.
//!
//! Like the Markdown viewer payload (`launch.rs`), the file is created
//! under the OS temp dir with `create_new` (no clobber) and mode 0o600 on
//! Unix. UNLIKE the Markdown payload, the child **deletes the file after
//! a successful read**: each payload holds raw RGBA (tens of MB for a
//! large image), so leaving one file per displayed image would exhaust
//! `/tmp` (tmpfs) over a long session. Failed reads keep the file for
//! post-mortem inspection (and reboot GC still applies).

use std::io::{Read as _, Write as _};
use std::path::PathBuf;

/// JSON header line preceding the raw RGBA bytes.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
struct Header {
    width: u32,
    height: u32,
    /// Resolved UI theme token (`"light"` | `"dark"` | `"system"`).
    #[serde(default)]
    theme: String,
    /// Resolved accent preset token (`"purple"` | `"pink"` | …).
    #[serde(default)]
    preset: String,
    /// UI chrome font family (title-bar text).
    #[serde(default, rename = "uiFontFamily")]
    ui_font_family: String,
}

/// Chrome appearance the parent passes to the viewer child — the
/// parent-side resolved equivalents of `ui_theme` / `ui_theme_preset` /
/// `ui_font_family`, as lowercase wire tokens.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ViewerChrome {
    pub theme: String,
    pub preset: String,
    pub ui_font_family: String,
}

/// A decoded RGBA image (plus chrome appearance) read back from a
/// payload file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImagePayload {
    pub width: u32,
    pub height: u32,
    /// `width * height * 4` bytes, un-premultiplied RGBA8.
    pub rgba: Vec<u8>,
    pub chrome: ViewerChrome,
}

/// Reject absurd headers before allocating (a corrupt/forged header must
/// not OOM the child). 16384² × 4 = 1 GiB is far above any real terminal
/// image (the parent store is quota-capped well below this anyway).
const MAX_DIM: u32 = 16384;

/// The JSON header line must fit in this many bytes. Bounds the header
/// scan so a forged payload cannot make the child buffer an unbounded
/// "first line".
const MAX_HEADER_BYTES: usize = 4096;

/// Serialize one image (+ chrome appearance) to a uniquely named temp
/// file and return its path. Fails if `rgba.len() != width * height * 4`.
pub fn write_image_payload(
    width: u32,
    height: u32,
    rgba: &[u8],
    chrome: &ViewerChrome,
) -> std::io::Result<PathBuf> {
    let expected = rgba_byte_count(width, height);
    let size_ok = matches!(expected, Some(e) if rgba.len() == e);
    if !size_ok || width == 0 || height == 0 || width > MAX_DIM || height > MAX_DIM {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!(
                "image payload size mismatch: {}x{} expects {:?} bytes, got {}",
                width,
                height,
                expected,
                rgba.len()
            ),
        ));
    }
    let header = serde_json::to_string(&Header {
        width,
        height,
        theme: chrome.theme.clone(),
        preset: chrome.preset.clone(),
        ui_font_family: chrome.ui_font_family.clone(),
    })
    .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    if header.len() + 1 > MAX_HEADER_BYTES {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!(
                "image payload header too large: {} bytes (cap {MAX_HEADER_BYTES})",
                header.len() + 1
            ),
        ));
    }
    let path = temp_payload_path();
    let mut f = open_create_new(&path)?;
    f.write_all(header.as_bytes())?;
    f.write_all(b"\n")?;
    f.write_all(rgba)?;
    Ok(path)
}

/// Read a payload file written by [`write_image_payload`], then delete
/// it (success path only — see the module doc's `/tmp` rationale).
///
/// Validation happens BEFORE the bulk allocation: the header is scanned
/// within a [`MAX_HEADER_BYTES`] bound, the dimensions are range-checked,
/// and the file's total size must equal `header + 1 + width*height*4`
/// exactly. A forged or truncated payload is rejected without ever
/// buffering the whole file.
pub fn read_image_payload(path: &std::path::Path) -> std::io::Result<ImagePayload> {
    let mut f = std::fs::File::open(path)?;

    // Bounded header scan: only the first MAX_HEADER_BYTES are buffered.
    let mut head = Vec::with_capacity(MAX_HEADER_BYTES);
    (&mut f)
        .take(MAX_HEADER_BYTES as u64)
        .read_to_end(&mut head)?;
    let nl = head
        .iter()
        .position(|b| *b == b'\n')
        .ok_or_else(|| invalid("image payload missing header line within the header cap"))?;
    let header: Header = serde_json::from_slice(&head[..nl])
        .map_err(|e| invalid(&format!("image payload header parse failed: {e}")))?;
    if header.width == 0 || header.height == 0 || header.width > MAX_DIM || header.height > MAX_DIM
    {
        return Err(invalid(&format!(
            "image payload dimensions out of range: {}x{}",
            header.width, header.height
        )));
    }
    let expected = rgba_byte_count(header.width, header.height)
        .ok_or_else(|| invalid("image payload dimensions overflow"))?;

    // Exact-size check against file metadata BEFORE allocating the RGBA
    // buffer, so a wrong-sized file never costs the full allocation.
    let total_expected = (nl as u64) + 1 + (expected as u64);
    let file_len = f.metadata()?.len();
    if file_len != total_expected {
        return Err(invalid(&format!(
            "image payload RGBA size mismatch: {}x{} expects {} total bytes, file has {}",
            header.width, header.height, total_expected, file_len
        )));
    }

    // Assemble the RGBA: the part already buffered past the newline,
    // then read_exact for the remainder.
    let mut rgba = Vec::with_capacity(expected);
    rgba.extend_from_slice(&head[nl + 1..]);
    let remaining = expected - rgba.len();
    if remaining > 0 {
        let start = rgba.len();
        rgba.resize(expected, 0);
        f.read_exact(&mut rgba[start..])?;
    }

    // Success: the bytes are in memory; drop the (potentially tens-of-MB)
    // temp file so a long session can't fill /tmp. Best-effort — a failed
    // unlink only costs disk, never correctness.
    if let Err(e) = std::fs::remove_file(path) {
        log::debug!("image payload: unlink after read failed: {e}");
    }

    Ok(ImagePayload {
        width: header.width,
        height: header.height,
        rgba,
        chrome: ViewerChrome {
            theme: header.theme,
            preset: header.preset,
            ui_font_family: header.ui_font_family,
        },
    })
}

fn invalid(msg: &str) -> std::io::Error {
    std::io::Error::new(std::io::ErrorKind::InvalidData, msg)
}

/// `width * height * 4` with overflow checking. `MAX_DIM` keeps the
/// product inside `usize` on every supported target today; the checked
/// arithmetic is defense-in-depth so a future `MAX_DIM` bump cannot
/// silently wrap on a 32-bit target (wrapping would accept a short
/// buffer as a "full" image).
fn rgba_byte_count(width: u32, height: u32) -> Option<usize> {
    (width as u64)
        .checked_mul(height as u64)
        .and_then(|n| n.checked_mul(4))
        .and_then(|n| usize::try_from(n).ok())
}

/// `create_new` + 0o600 on Unix (other local users must not read the
/// image), plain `create_new` elsewhere. Mirrors `launch::write_payload`.
/// Shared with the data-viewer payload (`viewer::data_payload`).
pub(crate) fn open_create_new(path: &std::path::Path) -> std::io::Result<std::fs::File> {
    let mut opts = std::fs::OpenOptions::new();
    opts.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt as _;
        opts.mode(0o600);
    }
    opts.open(path)
}

/// Unique temp path: PID + wall-clock nanos + monotonic counter (same
/// scheme as the Markdown viewer payload, different prefix/extension).
fn temp_payload_path() -> PathBuf {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let pid = std::process::id();
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    std::env::temp_dir().join(format!("emterm-image-viewer-{pid}-{nanos}-{n}.bin"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cleanup(path: &std::path::Path) {
        let _ = std::fs::remove_file(path);
    }

    fn chrome() -> ViewerChrome {
        ViewerChrome {
            theme: "dark".to_string(),
            preset: "pink".to_string(),
            ui_font_family: "Noto Sans JP".to_string(),
        }
    }

    #[test]
    fn payload_round_trips_with_chrome() {
        let rgba: Vec<u8> = (0..3 * 2 * 4).map(|i| i as u8).collect();
        let path = write_image_payload(3, 2, &rgba, &chrome()).unwrap();
        let back = read_image_payload(&path).unwrap();
        cleanup(&path);
        assert_eq!(back.width, 3);
        assert_eq!(back.height, 2);
        assert_eq!(back.rgba, rgba);
        assert_eq!(back.chrome, chrome());
    }

    #[test]
    fn successful_read_unlinks_the_payload_file() {
        let rgba = vec![0u8; 16];
        let path = write_image_payload(2, 2, &rgba, &chrome()).unwrap();
        let _ = read_image_payload(&path).unwrap();
        assert!(!path.exists(), "payload file must be removed after read");
    }

    #[test]
    fn failed_read_keeps_the_payload_file() {
        let rgba = vec![0u8; 16];
        let path = write_image_payload(2, 2, &rgba, &chrome()).unwrap();
        // Truncate below the expected RGBA length → read fails…
        let bytes = std::fs::read(&path).unwrap();
        std::fs::write(&path, &bytes[..bytes.len() - 4]).unwrap();
        assert!(read_image_payload(&path).is_err());
        // …and the file stays for post-mortem inspection.
        assert!(path.exists());
        cleanup(&path);
    }

    #[test]
    fn header_without_appearance_fields_defaults_to_empty() {
        // Forward/backward compat: serde defaults keep an old-format
        // header readable.
        let path = temp_payload_path();
        let mut bytes = b"{\"width\":1,\"height\":1}\n".to_vec();
        bytes.extend_from_slice(&[0u8; 4]);
        std::fs::write(&path, &bytes).unwrap();
        let back = read_image_payload(&path).unwrap();
        assert_eq!(back.chrome, ViewerChrome::default());
    }

    #[test]
    fn write_rejects_size_mismatch() {
        let err = write_image_payload(2, 2, &[0u8; 15], &chrome()).unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::InvalidInput);
    }

    #[test]
    fn write_rejects_zero_dimensions() {
        let err = write_image_payload(0, 2, &[], &chrome()).unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::InvalidInput);
    }

    #[test]
    fn write_rejects_oversized_header() {
        let big = ViewerChrome {
            ui_font_family: "x".repeat(MAX_HEADER_BYTES),
            ..chrome()
        };
        let err = write_image_payload(2, 2, &[0u8; 16], &big).unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::InvalidInput);
    }

    #[test]
    fn read_rejects_truncated_rgba_without_allocating() {
        let rgba = vec![0u8; 16];
        let path = write_image_payload(2, 2, &rgba, &chrome()).unwrap();
        let bytes = std::fs::read(&path).unwrap();
        std::fs::write(&path, &bytes[..bytes.len() - 4]).unwrap();
        let err = read_image_payload(&path).unwrap_err();
        cleanup(&path);
        assert_eq!(err.kind(), std::io::ErrorKind::InvalidData);
    }

    #[test]
    fn read_rejects_oversized_file() {
        let rgba = vec![0u8; 16];
        let path = write_image_payload(2, 2, &rgba, &chrome()).unwrap();
        // Append junk past the expected end → exact-size check fails.
        let mut bytes = std::fs::read(&path).unwrap();
        bytes.extend_from_slice(b"junk");
        std::fs::write(&path, &bytes).unwrap();
        let err = read_image_payload(&path).unwrap_err();
        cleanup(&path);
        assert_eq!(err.kind(), std::io::ErrorKind::InvalidData);
    }

    #[test]
    fn read_rejects_missing_header_newline() {
        let path = temp_payload_path();
        std::fs::write(&path, b"{\"width\":1,\"height\":1}").unwrap();
        let err = read_image_payload(&path).unwrap_err();
        cleanup(&path);
        assert_eq!(err.kind(), std::io::ErrorKind::InvalidData);
    }

    #[test]
    fn read_rejects_header_line_exceeding_cap() {
        // No newline within MAX_HEADER_BYTES → rejected by the bounded
        // scan even though the file is huge.
        let path = temp_payload_path();
        let mut bytes = vec![b'{'; MAX_HEADER_BYTES + 16];
        bytes.push(b'\n');
        std::fs::write(&path, &bytes).unwrap();
        let err = read_image_payload(&path).unwrap_err();
        cleanup(&path);
        assert_eq!(err.kind(), std::io::ErrorKind::InvalidData);
    }

    #[test]
    fn read_rejects_oversized_header_dims_without_allocating() {
        let path = temp_payload_path();
        std::fs::write(&path, b"{\"width\":99999,\"height\":99999}\n").unwrap();
        let err = read_image_payload(&path).unwrap_err();
        cleanup(&path);
        assert_eq!(err.kind(), std::io::ErrorKind::InvalidData);
    }

    #[test]
    fn rgba_byte_count_checks_overflow() {
        assert_eq!(rgba_byte_count(3, 2), Some(24));
        // MAX_DIM² × 4 = exactly 1 GiB — the largest accepted product.
        assert_eq!(rgba_byte_count(MAX_DIM, MAX_DIM), Some(1024 * 1024 * 1024));
        // u32::MAX² × 4 overflows u64 → must be None, never a wrapped
        // small value.
        assert_eq!(rgba_byte_count(u32::MAX, u32::MAX), None);
    }

    #[test]
    fn temp_paths_are_unique() {
        assert_ne!(temp_payload_path(), temp_payload_path());
    }
}
