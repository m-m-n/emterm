//! Parent-side viewer launcher (Phase 4).
//!
//! Bridges a completed [`RenderRequest`] to a separate child viewer
//! process. The render payload (markdown, format, basedir, resolved
//! appearance) is serialized to a JSON temp file under `/tmp`; the child
//! is then spawned as `self --viewer <temp-path>` and reads the payload on
//! startup. The spawn boundary is abstracted behind [`SpawnFn`] so the
//! parent→child translation is unit-testable without launching processes.

use std::path::{Path, PathBuf};

use crate::settings::{MarkdownAppearance, Settings, UiTheme, UiThemePreset};

use super::{MarkdownFormat, RenderRequest};

/// Serializable render payload written to the temp file and read by the
/// child. Field encodings match the TS viewer bundle's `ViewerPayload`
/// (lowercase theme/preset/format tokens).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ViewerPayload {
    /// Reassembled UTF-8 Markdown source.
    pub markdown: String,
    /// Source dialect (`"commonmark"` | `"gfm"`).
    pub format: String,
    /// Optional base directory for relative image resolution.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub basedir: Option<String>,
    /// Resolved viewer appearance.
    pub appearance: PayloadAppearance,
}

/// Appearance subset of [`ViewerPayload`], matching the TS
/// `ViewerAppearance` shape (camelCase font keys, lowercase enums).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct PayloadAppearance {
    /// Effective brightness mode (`"light"` | `"dark"` | `"system"`).
    pub theme: String,
    /// Effective accent preset (`"purple"` | `"blue"` | …).
    pub preset: String,
    /// Body font family (empty → CSS fallback chain).
    #[serde(rename = "bodyFontFamily")]
    pub body_font_family: String,
    /// Code font family.
    #[serde(rename = "codeFontFamily")]
    pub code_font_family: String,
    /// Emoji font family.
    #[serde(rename = "emojiFontFamily")]
    pub emoji_font_family: String,
    /// Base font size in pt.
    #[serde(rename = "fontSize")]
    pub font_size: u32,
}

/// Lowercase wire token for a [`UiTheme`] (matches the TS `UiTheme`).
fn theme_token(t: UiTheme) -> &'static str {
    match t {
        UiTheme::Light => "light",
        UiTheme::Dark => "dark",
        UiTheme::System => "system",
    }
}

/// Lowercase wire token for a [`UiThemePreset`] (matches TS `UiThemePreset`).
fn preset_token(p: UiThemePreset) -> &'static str {
    match p {
        UiThemePreset::Purple => "purple",
        UiThemePreset::Blue => "blue",
        UiThemePreset::Green => "green",
        UiThemePreset::Orange => "orange",
        UiThemePreset::Pink => "pink",
    }
}

impl PayloadAppearance {
    /// Build from a resolved [`MarkdownAppearance`] (Phase 1).
    pub fn from_appearance(a: &MarkdownAppearance) -> Self {
        Self {
            theme: theme_token(a.theme).to_string(),
            preset: preset_token(a.preset).to_string(),
            body_font_family: a.body_font_family.clone(),
            code_font_family: a.code_font_family.clone(),
            emoji_font_family: a.emoji_font_family.clone(),
            font_size: a.font_size,
        }
    }
}

impl ViewerPayload {
    /// Combine a completed [`RenderRequest`] (by value) with the resolved
    /// appearance from `settings` into a serializable payload, MOVING
    /// `markdown`/`format`/`basedir` out of the owned request so a
    /// tens-of-MiB document is not duplicated (H4).
    pub fn from_request(request: RenderRequest, settings: &Settings) -> Self {
        let appearance = PayloadAppearance::from_appearance(&settings.markdown_appearance());
        Self {
            format: request.format.as_str().to_string(),
            markdown: request.markdown,
            basedir: request.basedir,
            appearance,
        }
    }

    /// Parse the `format` token back into a [`MarkdownFormat`] (permissive,
    /// matching the bundle's default-to-commonmark behavior). The child
    /// passes the raw token to the TS bundle; this is the Rust-side reader
    /// used by the round-trip test and any future native rendering path.
    #[allow(dead_code)]
    pub fn markdown_format(&self) -> MarkdownFormat {
        MarkdownFormat::parse(&self.format)
    }

    /// Serialize to a pretty JSON string for the temp file.
    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string(self)
    }

    /// Deserialize from the JSON read out of the temp file.
    pub fn from_json(s: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(s)
    }
}

/// Serialize `payload` to a uniquely named temp file under the OS temp dir
/// and return its path. The file is intentionally *not* removed here — the
/// child reads it on startup, and `/tmp` is cleared on reboot (project
/// temp-file convention).
///
/// The file is created with `create_new(true)` (no clobber) and, on Unix,
/// with mode 0o600 (owner-read/write only) to prevent other local users from
/// reading the payload.
pub fn write_payload(payload: &ViewerPayload) -> std::io::Result<PathBuf> {
    use std::io::Write as _;
    let json = payload
        .to_json()
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    let path = temp_payload_path();
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt as _;
        let mut f = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&path)?;
        f.write_all(json.as_bytes())?;
    }
    #[cfg(not(unix))]
    {
        let mut f = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)?;
        f.write_all(json.as_bytes())?;
    }
    Ok(path)
}

/// Serialize `payload` to a temp file and hand its path to `spawn`. Returns
/// the spawned child PID. On any serialization / IO / spawn error, logs at
/// `warn` and returns the error (ERR_SPAWN): the terminal is unaffected.
///
/// Generic over the spawn closure so the parent→child translation is
/// unit-testable without launching real processes (`ProcessViewerSink`
/// does the production spawn directly via `write_payload` + `Command`, so
/// this entry point is exercised only by the launch unit tests).
#[allow(dead_code)]
pub fn launch_with<F>(payload: &ViewerPayload, mut spawn: F) -> std::io::Result<u32>
where
    F: FnMut(&Path) -> std::io::Result<u32>,
{
    let path = write_payload(payload)?;
    match spawn(&path) {
        Ok(pid) => {
            log::warn!("viewer: spawned child pid={pid} payload={}", path.display());
            Ok(pid)
        }
        Err(e) => {
            log::warn!("viewer: failed to spawn child ({e}); terminal unaffected");
            Err(e)
        }
    }
}

/// Build a unique payload temp-file path under the OS temp dir. Uses the
/// PID + a monotonic counter to avoid collisions between concurrent
/// viewers without requiring a temp-file crate.
fn temp_payload_path() -> PathBuf {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let pid = std::process::id();
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    std::env::temp_dir().join(format!("emterm-viewer-{pid}-{nanos}-{n}.json"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::Settings;

    fn sample_request() -> RenderRequest {
        RenderRequest {
            markdown: "# Hi\n\n本文 🎉".to_string(),
            format: MarkdownFormat::Gfm,
            basedir: Some("/home/me/docs".to_string()),
        }
    }

    #[test]
    fn payload_round_trips_through_json() {
        let settings = Settings::default();
        let payload = ViewerPayload::from_request(sample_request(), &settings);
        let json = payload.to_json().unwrap();
        let back = ViewerPayload::from_json(&json).unwrap();
        assert_eq!(payload, back);
        assert_eq!(back.markdown, "# Hi\n\n本文 🎉");
        assert_eq!(back.format, "gfm");
        assert_eq!(back.markdown_format(), MarkdownFormat::Gfm);
        assert_eq!(back.basedir.as_deref(), Some("/home/me/docs"));
    }

    #[test]
    fn payload_uses_lowercase_enum_tokens_and_camel_font_keys() {
        let settings = Settings::default(); // follow_ui=true → system/purple
        let payload = ViewerPayload::from_request(sample_request(), &settings);
        assert_eq!(payload.appearance.theme, "system");
        assert_eq!(payload.appearance.preset, "purple");
        let json = payload.to_json().unwrap();
        // TS bundle expects camelCase font keys.
        assert!(json.contains("\"bodyFontFamily\""));
        assert!(json.contains("\"codeFontFamily\""));
        assert!(json.contains("\"emojiFontFamily\""));
        assert!(json.contains("\"fontSize\""));
    }

    #[test]
    fn payload_reflects_follow_ui_false_theme_source() {
        let mut settings = Settings::default();
        settings.markdown_theme_follow_ui = false;
        settings.markdown_theme = UiTheme::Light;
        settings.markdown_theme_preset = UiThemePreset::Green;
        let payload = ViewerPayload::from_request(sample_request(), &settings);
        assert_eq!(payload.appearance.theme, "light");
        assert_eq!(payload.appearance.preset, "green");
    }

    #[test]
    fn basedir_omitted_when_none() {
        let mut req = sample_request();
        req.basedir = None;
        let payload = ViewerPayload::from_request(req, &Settings::default());
        let json = payload.to_json().unwrap();
        assert!(!json.contains("basedir"));
        let back = ViewerPayload::from_json(&json).unwrap();
        assert_eq!(back.basedir, None);
    }

    #[test]
    fn launch_with_writes_payload_and_invokes_spawn_once() {
        use std::cell::RefCell;
        let payload = ViewerPayload::from_request(sample_request(), &Settings::default());
        let captured: RefCell<Vec<PathBuf>> = RefCell::new(Vec::new());
        let spawn = |path: &Path| -> std::io::Result<u32> {
            // The payload file must exist and contain the markdown when the
            // spawner runs (the child will read it).
            let contents = std::fs::read_to_string(path)?;
            assert!(contents.contains("# Hi"));
            captured.borrow_mut().push(path.to_path_buf());
            Ok(4242)
        };
        let pid = launch_with(&payload, spawn).unwrap();
        assert_eq!(pid, 4242);
        let paths = captured.into_inner();
        assert_eq!(paths.len(), 1);
        // Clean up the temp file we created (test hygiene; production leaves
        // it for the child + reboot GC).
        let _ = std::fs::remove_file(&paths[0]);
    }

    #[test]
    fn launch_with_propagates_spawn_error() {
        let payload = ViewerPayload::from_request(sample_request(), &Settings::default());
        let spawn = |_path: &Path| -> std::io::Result<u32> {
            Err(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "no binary",
            ))
        };
        let err = launch_with(&payload, spawn).unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::NotFound);
    }

    #[test]
    fn temp_paths_are_unique() {
        let a = temp_payload_path();
        let b = temp_payload_path();
        assert_ne!(a, b);
    }

    /// H5: the child-viewer wire payload is defined twice (Rust here, TS in
    /// `native-poc/viewer/web/entry.ts`). Both sides validate against the
    /// committed fixture so a field-name rename/removal on either side is a
    /// failing test. The TS half lives in `entry.test.ts`.
    #[test]
    fn shared_fixture_deserializes_with_expected_fields() {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/viewer/web/__fixtures__/payload.fixture.json"
        );
        let raw =
            std::fs::read_to_string(path).unwrap_or_else(|e| panic!("read fixture {path}: {e}"));
        let payload = ViewerPayload::from_json(&raw).expect("fixture deserializes");

        assert_eq!(payload.markdown, "# Hi\n\n本文 🎉");
        assert_eq!(payload.format, "gfm");
        assert_eq!(payload.basedir.as_deref(), Some("/home/me/docs"));
        assert_eq!(payload.appearance.theme, "dark");
        assert_eq!(payload.appearance.preset, "purple");
        assert_eq!(payload.appearance.body_font_family, "Noto Sans");
        assert_eq!(payload.appearance.code_font_family, "Fira Code");
        assert_eq!(payload.appearance.emoji_font_family, "Noto Color Emoji");
        assert_eq!(payload.appearance.font_size, 14);
    }
}
