//! Child `--viewer` window (Linux GTK / WebKitGTK via wry).
//!
//! This module owns the *separate viewer process* on Linux: it initializes
//! GTK, builds a `gtk::Window`, mounts a wry `WebView` that serves the
//! embedded Markdown viewer bundle through a custom URI scheme, injects the
//! render payload, and drives its own GTK main loop. The terminal process'
//! winit loop and `WindowHost` are untouched — closing this window exits
//! only the child (FR3 / FR9).
//!
//! Phase 5 responsibilities also live here: navigation interception
//! (FR7), the basedir-confined custom-scheme image resolver (FR8), and
//! `Esc` / `q` / close-button handling (FR9). The security-critical pure
//! logic (link gate, image confinement) lives in `crate::links` and
//! `crate::viewer::image_resolver` so it is unit-tested without a window.

#![cfg(target_os = "linux")]

use std::borrow::Cow;

use gtk::prelude::*;
use gtk::{Window, WindowType};
use wry::http::{Request, Response};
use wry::{WebViewBuilder, WebViewBuilderExtUnix};

use super::assets;
use super::image_resolver;
use super::launch::ViewerPayload;

/// Custom URI scheme the child serves its own content from. WebKitGTK
/// origins look like `emterm-viewer://<host>/<path>`.
const SCHEME: &str = "emterm-viewer";
/// Host used for in-bundle asset requests (`emterm-viewer://localhost/…`).
const HOST: &str = "localhost";
/// Path prefix for basedir-relative image requests.
const IMAGE_PREFIX: &str = "/__img/";

/// Run the child viewer for the payload at `payload_path`. Blocks until the
/// window closes, then returns. Any setup failure logs at `warn`/`error`
/// (ERR_SPAWN side) and returns an error so the child can exit non-zero.
pub fn run(payload_path: &str) -> Result<(), String> {
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

    gtk::init().map_err(|e| format!("viewer: gtk init failed: {e}"))?;

    let window = Window::new(WindowType::Toplevel);
    window.set_title("eMterm Markdown Viewer");
    window.set_default_size(960, 720);

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

    let builder = WebViewBuilder::new()
        .with_url(format!("{SCHEME}://{HOST}/{}", assets::INDEX_PATH))
        .with_initialization_script(&init_script)
        .with_custom_protocol(SCHEME.to_string(), move |_id, request| {
            handle_request(&request, basedir.as_deref())
        })
        // FR7: deny in-window navigation; route safe external URIs to the OS.
        .with_navigation_handler(|uri| navigation_allowed(&uri));

    let _webview = builder
        .build_gtk(&window)
        .map_err(|e| format!("viewer: webview build failed: {e}"))?;

    // FR9: window close button exits the child loop.
    let running = std::rc::Rc::new(std::cell::Cell::new(true));
    {
        let running = running.clone();
        window.connect_delete_event(move |_, _| {
            running.set(false);
            gtk::glib::Propagation::Proceed
        });
    }
    // FR9: Esc / q exit the child.
    {
        let running = running.clone();
        window.connect_key_press_event(move |_, ev| {
            let key = ev.keyval();
            if key == gtk::gdk::keys::constants::Escape
                || key == gtk::gdk::keys::constants::q
                || key == gtk::gdk::keys::constants::Q
            {
                running.set(false);
            }
            gtk::glib::Propagation::Proceed
        });
    }

    window.show_all();

    // Child-owned GTK main loop (the terminal's winit loop is separate).
    while running.get() {
        gtk::main_iteration_do(true);
    }
    Ok(())
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
/// load its own assets. Any other URI is *denied* in-window; if it is a
/// safe external scheme it is opened via the OS handler instead (FR7).
pub fn navigation_allowed(uri: &str) -> bool {
    if uri.starts_with(&format!("{SCHEME}://")) {
        return true;
    }
    // External link: hand to the OS if safe, never navigate the window.
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
        assert!(!navigation_allowed("https://example.com"));
        assert!(!navigation_allowed("file:///etc/passwd"));
        assert!(!navigation_allowed("javascript:alert(1)"));
    }

    #[test]
    fn percent_decode_handles_spaces_and_passthrough() {
        assert_eq!(percent_decode("a%20b.png"), "a b.png");
        assert_eq!(percent_decode("plain.png"), "plain.png");
        assert_eq!(percent_decode("100%done"), "100%done");
    }
}
