//! Child `--viewer` window.
//!
//! Runs the Markdown viewer in a separate child process via the shared
//! [`crate::webview_host`] runtime: GTK + WebKitGTK on Linux, winit +
//! WebView2 on Windows. The terminal's winit loop and `WindowHost` are
//! untouched — closing this window exits only the child (FR3 / FR9).
//!
//! Phase 5 responsibilities also live here: navigation interception
//! (FR7), the basedir-confined custom-scheme image resolver (FR8), and
//! `Esc` / `q` / close-button handling (FR9). The security-critical pure
//! logic (link gate, image confinement) lives in `crate::links` and
//! `crate::viewer::image_resolver` so it is unit-tested without a window.

use std::borrow::Cow;

use wry::http::{Request, Response};

use super::assets;
use super::image_resolver;
use super::launch::ViewerPayload;

/// Custom URI scheme the child serves its own content from. WebView
/// origins look like `emterm-viewer://<host>/<path>`.
const SCHEME: &str = "emterm-viewer";
/// Host used for in-bundle asset requests (`emterm-viewer://localhost/…`).
const HOST: &str = "localhost";
/// Path prefix for basedir-relative image requests.
const IMAGE_PREFIX: &str = "/__img/";
/// FR2: the Markdown viewer opens maximized; its `initial_size` is kept
/// as the restore size. A `const` so the maximize-on-launch decision is a
/// deterministic, unit-testable fact even though the host itself carries
/// payload-derived closures that are awkward to build in a unit test.
const MAXIMIZED: bool = true;

/// Run the child viewer for the payload at `payload_path`. Blocks until the
/// window closes, then returns. Any setup failure logs at `warn`/`error`
/// (ERR_SPAWN side) and returns an error so the child can exit non-zero.
pub fn run(payload_path: &str) -> Result<(), String> {
    use crate::webview_host::WebViewHost;

    let raw = std::fs::read_to_string(payload_path)
        .map_err(|e| format!("viewer: cannot read payload {payload_path}: {e}"))?;
    let payload =
        ViewerPayload::from_json(&raw).map_err(|e| format!("viewer: bad payload JSON: {e}"))?;

    // M2: the payload (full document text) is now in memory; delete the temp
    // file immediately so it doesn't sit in the OS temp dir until reboot.
    // Best-effort — a failure to remove it is non-fatal.
    let _ = std::fs::remove_file(payload_path);

    if !assets::is_embedded() {
        return Err("viewer: bundle not embedded (run `bun run build:viewer`)".to_string());
    }

    // The injected payload global + a tiny ready-signal, evaluated before
    // the bundle entry runs. The bundle reads `window.__EMTERM_VIEWER_PAYLOAD__`.
    let payload_json = payload
        .to_json()
        .map_err(|e| format!("viewer: re-serialize payload failed: {e}"))?;
    // M5: U+2028 (LINE SEPARATOR) / U+2029 (PARAGRAPH SEPARATOR) are valid
    // inside JSON strings but were JS line terminators before ES2019, so
    // they can break the injected `window.__EMTERM_VIEWER_PAYLOAD__ = …;`
    // statement. Re-escape them to their `\uXXXX` JS form before
    // interpolating the JSON into the script source.
    let payload_json = payload_json
        .replace('\u{2028}', "\\u2028")
        .replace('\u{2029}', "\\u2029");
    let init_script = format!(
        "window.__EMTERM_VIEWER_PAYLOAD__ = {payload_json};\
         window.dispatchEvent(new Event('emterm-viewer-payload'));"
    );

    // basedir captured by the custom-scheme image resolver.
    let basedir = payload.basedir.clone();

    let host = WebViewHost {
        scheme: SCHEME.to_string(),
        host: HOST.to_string(),
        title: "eMterm Markdown Viewer".to_string(),
        initial_size: (960.0, 720.0),
        initial_url_path: assets::INDEX_PATH.to_string(),
        init_script: Some(init_script),
        request_handler: Box::new(move |request| handle_request(request, basedir.as_deref())),
        navigation_handler: Box::new(|uri| handle_navigation(uri)),
        // The Markdown viewer doesn't open popups; the HTML viewer
        // (`viewer::html_window`) is the first caller of this hook.
        new_window_handler: None,
        ipc: None,
        // FR9: Esc / q exit the read-only viewer.
        close_on_esc_q: true,
        // FR2: open maximized; `initial_size` is the restore size.
        maximized: MAXIMIZED,
    };
    host.run()
}

/// Custom-scheme request router: serves in-bundle assets and, under
/// [`IMAGE_PREFIX`], basedir-confined local images (FR8).
fn handle_request(
    request: &Request<Vec<u8>>,
    basedir: Option<&str>,
) -> Response<Cow<'static, [u8]>> {
    let uri = request.uri();
    let path = uri.path();

    if let Some(rel) = path.strip_prefix(IMAGE_PREFIX) {
        return serve_image(rel, basedir);
    }

    match assets::asset(path) {
        Some((bytes, content_type)) => Response::builder()
            .status(200)
            .header("Content-Type", content_type)
            // M3: prevent MIME sniffing into the privileged viewer origin.
            .header("X-Content-Type-Options", "nosniff")
            .body(Cow::Borrowed(bytes))
            .unwrap_or_else(|_| not_found()),
        None => not_found(),
    }
}

/// Resolve a basedir-relative image request through the confined resolver.
fn serve_image(rel: &str, basedir: Option<&str>) -> Response<Cow<'static, [u8]>> {
    // The path may be percent-encoded by the WebView; decode minimally.
    let decoded = percent_decode(rel);
    match image_resolver::resolve_image(basedir, &decoded) {
        Ok((bytes, mime)) => Response::builder()
            .status(200)
            .header("Content-Type", mime)
            // M3: prevent MIME sniffing into the privileged viewer origin.
            .header("X-Content-Type-Options", "nosniff")
            .body(Cow::Owned(bytes))
            .unwrap_or_else(|_| not_found()),
        Err(e) => {
            log::warn!("viewer: image request denied ({decoded:?}): {e:?}");
            Response::builder()
                .status(403)
                .body(Cow::Borrowed(&b"forbidden"[..]))
                .unwrap_or_else(|_| not_found())
        }
    }
}

/// Decide whether the WebView may navigate to `uri` in-window.
///
/// In-bundle (`emterm-viewer://`) navigation is allowed so the page can
/// load its own assets. WebView2 on Windows cannot register non-standard
/// URI schemes, so wry rewrites `emterm-viewer://localhost/...` to
/// `http(s)://emterm-viewer.localhost/...`; the rewritten form is also
/// accepted (same workaround as the settings window). Any other URI is
/// denied in-window. This is a **pure predicate** with no side effects —
/// opening safe external URIs in the OS is the caller's job
/// ([`handle_navigation`]), so unit tests can assert the decision without
/// spawning a browser.
pub fn navigation_allowed(uri: &str) -> bool {
    uri.starts_with("emterm-viewer://")
        || uri.starts_with("http://emterm-viewer.localhost/")
        || uri.starts_with("https://emterm-viewer.localhost/")
}

/// Navigation handler for the WebView: allow in-window navigation only
/// for in-bundle URIs; for any other URI, deny in-window navigation and
/// hand a safe external scheme to the OS handler (FR7).
///
/// Returns whether the WebView may proceed in-window. The OS-open is a
/// side effect, deliberately kept out of [`navigation_allowed`] so the
/// predicate stays test-safe.
pub fn handle_navigation(uri: &str) -> bool {
    if navigation_allowed(uri) {
        return true;
    }
    crate::links::open_safe_uri(uri);
    false
}

fn not_found() -> Response<Cow<'static, [u8]>> {
    Response::builder()
        .status(404)
        .body(Cow::Borrowed(&b"not found"[..]))
        .expect("static 404 response")
}

/// Minimal percent-decoding for custom-scheme image paths (handles `%20`
/// etc.). Invalid escapes are left verbatim.
fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            let hi = (bytes[i + 1] as char).to_digit(16);
            let lo = (bytes[i + 2] as char).to_digit(16);
            if let (Some(h), Some(l)) = (hi, lo) {
                out.push((h * 16 + l) as u8);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn in_bundle_navigation_is_allowed() {
        assert!(navigation_allowed("emterm-viewer://localhost/index.html"));
        assert!(navigation_allowed("emterm-viewer://localhost/index-abc.js"));
    }

    #[test]
    fn external_navigation_is_denied_in_window() {
        // Safe or not, the window itself must never navigate externally.
        // `navigation_allowed` is a PURE predicate — calling it here must
        // NOT spawn an OS browser (the side effect lives in
        // `handle_navigation`, exercised only at runtime). Using a safe
        // https URL here previously launched xdg-open on every test run.
        assert!(!navigation_allowed("https://example.com"));
        assert!(!navigation_allowed("file:///etc/passwd"));
        assert!(!navigation_allowed("javascript:alert(1)"));
    }

    #[test]
    fn navigation_gate_accepts_webview2_workaround_form() {
        // WebView2 rewrites `emterm-viewer://localhost/...` to
        // `http(s)://emterm-viewer.localhost/...`; the rewritten form
        // must still count as in-bundle so wry's NavigationStarting
        // callback proceeds instead of being routed to the OS browser.
        assert!(navigation_allowed(
            "http://emterm-viewer.localhost/index.html"
        ));
        assert!(navigation_allowed(
            "https://emterm-viewer.localhost/assets/app.js"
        ));
        // Look-alike origins must still be rejected.
        assert!(!navigation_allowed(
            "http://emterm-viewer.localhost.evil.com/"
        ));
        assert!(!navigation_allowed("http://emterm-viewer/index.html"));
    }

    #[test]
    fn percent_decode_handles_spaces_and_passthrough() {
        assert_eq!(percent_decode("a%20b.png"), "a b.png");
        assert_eq!(percent_decode("plain.png"), "plain.png");
        assert_eq!(percent_decode("100%done"), "100%done");
    }

    // TS-1: the Markdown viewer opens maximized (FR2). The `run` host sets
    // `maximized: MAXIMIZED`; its `initial_size` (960×720) stays as the
    // restore size.
    #[test]
    fn markdown_viewer_opens_maximized() {
        assert!(MAXIMIZED);
    }
}
