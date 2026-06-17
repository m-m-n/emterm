//! Data-viewer (JSON/YAML) payload file format.
//!
//! The parent terminal writes one reassembled document to a temp file and
//! spawns `self --data-viewer <path>`; the child reads it back on
//! startup. Same shape as the image-viewer payload: a single-line JSON
//! header (format + chrome appearance) followed by the raw UTF-8 source
//! text:
//!
//! ```text
//! {"format":"json","theme":"dark","preset":"pink","uiFontFamily":"..."}\n
//! <raw UTF-8 document bytes>
//! ```
//!
//! Like the image payload, the file is created with `create_new` + 0o600
//! (Unix) and the child **deletes it after a successful read** — a
//! document can be tens of MB and the parent's reap removes the file if
//! the child dies before reading (`viewer::image` worker discipline does
//! not apply here; the markdown-style `ProcessViewerSink` spawns these,
//! so cleanup-on-success is the only unlink. Reboot GC covers failures).

use std::io::{Read as _, Write as _};
use std::path::PathBuf;

use super::data::DataFormat;
use super::image_payload::{open_create_new, ViewerChrome};

/// JSON header line preceding the raw document bytes.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
struct Header {
    /// `"json"` | `"yaml"`.
    format: String,
    #[serde(default)]
    theme: String,
    #[serde(default)]
    preset: String,
    #[serde(default, rename = "uiFontFamily")]
    ui_font_family: String,
}

/// A document (plus chrome appearance) read back from a payload file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DataPayload {
    pub format: DataFormat,
    /// Raw UTF-8 source text.
    pub text: String,
    pub chrome: ViewerChrome,
}

/// The JSON header line must fit in this many bytes (bounded scan).
const MAX_HEADER_BYTES: usize = 4096;

/// Cap on the document text accepted by the child. Matches the session
/// manager's cumulative cap (`markdown::MAX_SESSION_DATA_SIZE`) — the
/// parent can never legitimately write more than one session's worth.
const MAX_TEXT_BYTES: u64 = 100 * 1024 * 1024;

/// Serialize one document (+ chrome appearance) to a uniquely named temp
/// file and return its path.
pub fn write_data_payload(
    format: DataFormat,
    text: &str,
    chrome: &ViewerChrome,
) -> std::io::Result<PathBuf> {
    let header = serde_json::to_string(&Header {
        format: format.as_str().to_string(),
        theme: chrome.theme.clone(),
        preset: chrome.preset.clone(),
        ui_font_family: chrome.ui_font_family.clone(),
    })
    .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    if header.len() + 1 > MAX_HEADER_BYTES {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!(
                "data payload header too large: {} bytes (cap {MAX_HEADER_BYTES})",
                header.len() + 1
            ),
        ));
    }
    let path = temp_payload_path();
    let mut f = open_create_new(&path)?;
    f.write_all(header.as_bytes())?;
    f.write_all(b"\n")?;
    f.write_all(text.as_bytes())?;
    Ok(path)
}

/// Read a payload file written by [`write_data_payload`], then delete it
/// (success path only). The header is scanned within a bounded window
/// and the text size is capped before the bulk read.
pub fn read_data_payload(path: &std::path::Path) -> std::io::Result<DataPayload> {
    let mut f = std::fs::File::open(path)?;

    let mut head = Vec::with_capacity(MAX_HEADER_BYTES);
    (&mut f)
        .take(MAX_HEADER_BYTES as u64)
        .read_to_end(&mut head)?;
    let nl = head
        .iter()
        .position(|b| *b == b'\n')
        .ok_or_else(|| invalid("data payload missing header line within the header cap"))?;
    let header: Header = serde_json::from_slice(&head[..nl])
        .map_err(|e| invalid(&format!("data payload header parse failed: {e}")))?;
    let format = DataFormat::parse(&header.format)
        .ok_or_else(|| invalid(&format!("data payload unknown format {:?}", header.format)))?;

    // Size cap BEFORE the bulk read (fstat on the open handle).
    let file_len = f.metadata()?.len();
    let text_len = file_len.saturating_sub(nl as u64 + 1);
    if text_len > MAX_TEXT_BYTES {
        return Err(invalid(&format!(
            "data payload text too large: {text_len} bytes (cap {MAX_TEXT_BYTES})"
        )));
    }

    let mut bytes = Vec::with_capacity(text_len as usize);
    bytes.extend_from_slice(&head[nl + 1..]);
    f.read_to_end(&mut bytes)?;
    let text =
        String::from_utf8(bytes).map_err(|_| invalid("data payload text is not valid UTF-8"))?;

    // Success: drop the temp file (same rationale as the image payload).
    if let Err(e) = std::fs::remove_file(path) {
        log::debug!("data payload: unlink after read failed: {e}");
    }

    Ok(DataPayload {
        format,
        text,
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

/// Unique temp path (same scheme as the image payload, different prefix).
fn temp_payload_path() -> PathBuf {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let pid = std::process::id();
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    std::env::temp_dir().join(format!("emterm-data-viewer-{pid}-{nanos}-{n}.bin"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chrome() -> ViewerChrome {
        ViewerChrome {
            theme: "dark".to_string(),
            preset: "pink".to_string(),
            ui_font_family: "Noto Sans JP".to_string(),
        }
    }

    #[test]
    fn payload_round_trips_json() {
        let text = "{\"k\": [1, 2, 3], \"日本語\": true}";
        let path = write_data_payload(DataFormat::Json, text, &chrome()).unwrap();
        let back = read_data_payload(&path).unwrap();
        assert_eq!(back.format, DataFormat::Json);
        assert_eq!(back.text, text);
        assert_eq!(back.chrome, chrome());
        // Unlinked after a successful read.
        assert!(!path.exists());
    }

    #[test]
    fn payload_round_trips_yaml_with_newlines() {
        let text = "k: 1\nlist:\n  - a\n  - b\n";
        let path = write_data_payload(DataFormat::Yaml, text, &chrome()).unwrap();
        let back = read_data_payload(&path).unwrap();
        assert_eq!(back.format, DataFormat::Yaml);
        assert_eq!(back.text, text);
    }

    #[test]
    fn empty_document_round_trips() {
        let path = write_data_payload(DataFormat::Json, "", &chrome()).unwrap();
        let back = read_data_payload(&path).unwrap();
        assert_eq!(back.text, "");
    }

    #[test]
    fn read_rejects_unknown_format() {
        let path = temp_payload_path();
        std::fs::write(&path, b"{\"format\":\"toml\"}\nx").unwrap();
        let err = read_data_payload(&path).unwrap_err();
        let _ = std::fs::remove_file(&path);
        assert_eq!(err.kind(), std::io::ErrorKind::InvalidData);
    }

    #[test]
    fn read_rejects_invalid_utf8_text() {
        let path = temp_payload_path();
        let mut bytes = b"{\"format\":\"json\"}\n".to_vec();
        bytes.extend_from_slice(&[0xFF, 0xFE]);
        std::fs::write(&path, &bytes).unwrap();
        let err = read_data_payload(&path).unwrap_err();
        // Failed read keeps the file.
        assert!(path.exists());
        let _ = std::fs::remove_file(&path);
        assert_eq!(err.kind(), std::io::ErrorKind::InvalidData);
    }

    #[test]
    fn read_rejects_missing_header_newline() {
        let path = temp_payload_path();
        std::fs::write(&path, b"{\"format\":\"json\"}").unwrap();
        let err = read_data_payload(&path).unwrap_err();
        let _ = std::fs::remove_file(&path);
        assert_eq!(err.kind(), std::io::ErrorKind::InvalidData);
    }
}
