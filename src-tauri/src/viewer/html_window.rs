//! Child `--html-viewer` window (task0004).
//!
//! Runs the HTML viewer in a separate child process via the shared
//! [`crate::webview_host`] runtime (GTK + WebKitGTK on Linux, winit +
//! WebView2 on Windows) — same window layer as the Markdown viewer
//! (`viewer::window`), but the document is served **verbatim** as the root
//! resource of a dedicated custom scheme instead of being injected into a
//! bundled renderer (Decision D1/D2 in IMPLEMENTATION.md): no Markdown
//! parser, no `web-shared` stylesheet, no wrapper — the payload's raw HTML
//! bytes are the response body.
//!
//! Security policy (Decision D3 / FR4 / FR6 / NFR1):
//! - **Network isolation**: the document response carries a
//!   `Content-Security-Policy` header ([`build_csp`]) scoped to the
//!   viewer's own scheme (both the native `scheme://` form and the
//!   WebView2 `*.localhost` rewritten form) plus inline script/style and
//!   `data:` — no remote origin, `connect-src 'none'`. JavaScript
//!   execution itself stays enabled; isolation is enforced at the
//!   network/navigation/filesystem layers, not by sanitizing the document.
//! - **Filesystem**: every non-root request resolves through
//!   `viewer::html_resolver` against the payload's `basedir`; anything
//!   outside the confined subtree (or off the MIME allowlist) is denied
//!   with 403.
//! - **Navigation**: [`navigation_allowed`] mirrors the Markdown viewer's
//!   gate — only in-scheme URIs may navigate in-window; `http(s)` targets
//!   are handed to the OS's safe external-open helper
//!   (`crate::links::open_safe_uri`), everything else is dropped.
//! - **Popups**: the shared `webview_host` new-window hook
//!   ([`handle_new_window`]) never allows an in-WebView popup; `http(s)`
//!   targets get the same external delegation as denied navigation.
//!
//! The request handler, CSP builder, navigation predicate, and popup
//! decision are all side-effect-free (besides the resolver's file I/O) so
//! they are unit-tested without a WebView (same style as `viewer::window`).

use std::borrow::Cow;

use wry::http::{Request, Response};

use super::html::HtmlPayload;
use super::html_resolver;

/// Custom URI scheme the child serves the document and its basedir
/// resources from. WebView origins look like
/// `emterm-html-viewer://localhost/…`.
const SCHEME: &str = "emterm-html-viewer";
/// Host used for the document/resource requests.
const HOST: &str = "localhost";
/// Same window-management behavior as the Markdown viewer (REQUIREMENTS.md
/// 6.1): opens maximized; `initial_size` below is the restore size.
const MAXIMIZED: bool = true;

/// Run the child HTML viewer for the payload at `payload_path`. Blocks
/// until the window closes, then returns. A missing/unreadable payload is
/// reported as an `Err` (AC-1) so the caller can log and exit non-zero
/// without panicking.
pub fn run(payload_path: &str) -> Result<(), String> {
    use crate::webview_host::WebViewHost;

    let raw = std::fs::read_to_string(payload_path)
        .map_err(|e| format!("html viewer: cannot read payload {payload_path}: {e}"))?;
    let payload =
        HtmlPayload::from_json(&raw).map_err(|e| format!("html viewer: bad payload JSON: {e}"))?;

    // The payload (HTML text) is now in memory; delete the temp file
    // immediately so it doesn't sit in the OS temp dir until reboot.
    // Best-effort — a failure to remove it is non-fatal.
    let _ = std::fs::remove_file(payload_path);

    let html = payload.html;
    let basedir = payload.basedir;

    let host = WebViewHost {
        scheme: SCHEME.to_string(),
        host: HOST.to_string(),
        title: "eMterm HTML Viewer".to_string(),
        initial_size: (960.0, 720.0),
        // Root path — the document itself, not an in-bundle asset path.
        initial_url_path: String::new(),
        init_script: None,
        request_handler: Box::new(move |request| {
            handle_request(request, &html, basedir.as_deref())
        }),
        navigation_handler: Box::new(|uri| handle_navigation(uri)),
        new_window_handler: Some(Box::new(|uri| handle_new_window(uri))),
        ipc: None,
        // Read-only viewer — Esc/q closes it, same as the Markdown viewer.
        close_on_esc_q: true,
        maximized: MAXIMIZED,
    };
    host.run()
}

/// Custom-scheme request router: the root path serves the payload document
/// (AC-2); every other path resolves through the basedir resource resolver
/// (AC-3). Thin adapter over [`route_request`] so the WebViewHost callback
/// gets the real `wry::http::Request` while the pure routing logic stays
/// testable on a plain path string.
fn handle_request(
    request: &Request<Vec<u8>>,
    html: &str,
    basedir: Option<&str>,
) -> Response<Cow<'static, [u8]>> {
    route_request(request.uri().path(), html, basedir)
}

/// Pure request router (AC-2/AC-3): root path → the document response;
/// anything else → a basedir-confined resource lookup.
fn route_request(path: &str, html: &str, basedir: Option<&str>) -> Response<Cow<'static, [u8]>> {
    if path.is_empty() || path == "/" {
        return document_response(html);
    }

    let decoded = percent_decode(path.trim_start_matches('/'));
    serve_resource(&decoded, basedir)
}

/// Build the root document response: the payload HTML verbatim (no
/// wrapper, no injected styles) plus the CSP header and a nosniff guard
/// (AC-2).
fn document_response(html: &str) -> Response<Cow<'static, [u8]>> {
    Response::builder()
        .status(200)
        .header("Content-Type", "text/html; charset=utf-8")
        .header("X-Content-Type-Options", "nosniff")
        .header("Content-Security-Policy", build_csp(SCHEME))
        .body(Cow::Owned(html.as_bytes().to_vec()))
        .unwrap_or_else(|_| not_found())
}

/// Resolve a basedir-relative resource request through the confined
/// resolver (AC-3); denials return 403.
fn serve_resource(rel: &str, basedir: Option<&str>) -> Response<Cow<'static, [u8]>> {
    match html_resolver::resolve_resource(basedir, rel) {
        Ok((bytes, mime)) => Response::builder()
            .status(200)
            .header("Content-Type", mime)
            // Prevent MIME sniffing into the privileged viewer origin.
            .header("X-Content-Type-Options", "nosniff")
            .body(Cow::Owned(bytes))
            .unwrap_or_else(|_| not_found()),
        Err(e) => {
            log::warn!("html viewer: resource request denied ({rel:?}): {e:?}");
            forbidden()
        }
    }
}

/// Build the `Content-Security-Policy` header value for the document
/// response (Decision D3 / FR4 / NFR1): sources are restricted to the
/// viewer's own scheme (native form + both WebView2 `*.localhost`
/// rewritten forms, mirroring [`navigation_allowed`]'s D2 workaround),
/// inline script/style (the document's own `<script>`/`<style>`/`style=`
/// must still run — D3 enforces isolation at the network/navigation/
/// filesystem layers, not by stripping the document), and `data:` URIs.
/// `connect-src 'none'` blocks fetch/XHR/WebSocket outright.
fn build_csp(scheme: &str) -> String {
    let sources = format!("{scheme}: http://{scheme}.localhost https://{scheme}.localhost");
    format!(
        "default-src {sources} data:; \
         script-src {sources} data: 'unsafe-inline'; \
         style-src {sources} data: 'unsafe-inline'; \
         connect-src 'none'"
    )
}

/// Decide whether the WebView may navigate to `uri` in-window.
///
/// Only the viewer's own scheme may navigate in-window (both the native
/// form and the WebView2 `*.localhost` rewritten form — same D2 workaround
/// as the Markdown viewer's gate); any other URI (http(s), file,
/// javascript, data, ...) is denied. This is a **pure predicate** with no
/// side effects — opening safe external URIs in the OS is
/// [`handle_navigation`]'s job, so unit tests can assert the decision
/// without spawning a browser.
pub fn navigation_allowed(uri: &str) -> bool {
    uri.starts_with(&format!("{SCHEME}://"))
        || uri.starts_with(&format!("http://{SCHEME}.localhost/"))
        || uri.starts_with(&format!("https://{SCHEME}.localhost/"))
}

/// Whether a denied navigation/popup target should be handed to the OS's
/// safe external-open helper: `http(s)` targets only (D3 / FR6). Kept as a
/// pure decision, separate from the side-effecting `open_safe_uri` call, so
/// unit tests can assert the delegation without spawning a process (mirrors
/// `viewer::window`'s split between `navigation_allowed` and
/// `handle_navigation`).
pub fn is_external_open_target(uri: &str) -> bool {
    uri.starts_with("http://") || uri.starts_with("https://")
}

/// Navigation handler for the WebView: allow in-window navigation only for
/// in-scheme URIs; for any other URI, deny in-window navigation and, when
/// `http(s)`, hand it to the OS handler (FR6).
///
/// Returns whether the WebView may proceed in-window. The OS-open is a side
/// effect, deliberately kept out of [`navigation_allowed`] /
/// [`is_external_open_target`] so those predicates stay test-safe.
pub fn handle_navigation(uri: &str) -> bool {
    if navigation_allowed(uri) {
        return true;
    }
    if is_external_open_target(uri) {
        crate::links::open_safe_uri(uri);
    }
    false
}

/// New-window (popup) handler wired into `webview_host`'s
/// `new_window_handler` hook: the popup is ALWAYS denied in-WebView (the
/// host layer never inspects the return value — there isn't one); `http(s)`
/// targets are handed to the OS's safe external-open helper, same
/// delegation rule as denied navigation (D3).
pub fn handle_new_window(uri: &str) {
    if is_external_open_target(uri) {
        crate::links::open_safe_uri(uri);
    }
}

fn not_found() -> Response<Cow<'static, [u8]>> {
    Response::builder()
        .status(404)
        .body(Cow::Borrowed(&b"not found"[..]))
        .expect("static 404 response")
}

fn forbidden() -> Response<Cow<'static, [u8]>> {
    Response::builder()
        .status(403)
        .body(Cow::Borrowed(&b"forbidden"[..]))
        .expect("static 403 response")
}

/// Minimal percent-decoding for custom-scheme resource paths (handles
/// `%20` etc.). Invalid escapes are left verbatim. Mirrors
/// `viewer::window`'s helper.
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

    fn temp_dir(label: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "emterm-htmlwindow-{label}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    // ── AC-2: root path serves the payload HTML verbatim + CSP header ───

    #[test]
    fn root_path_serves_html_verbatim() {
        let html = "<html><body>Hello <b>World</b></body></html>";
        let resp = route_request("/", html, None);
        assert_eq!(resp.status(), 200);
        assert_eq!(resp.body().as_ref(), html.as_bytes());
    }

    #[test]
    fn empty_path_also_serves_the_document() {
        // The initial URL is built as `{scheme}://{host}/` (empty
        // `initial_url_path`); some WebView backends may report the path as
        // "" rather than "/". Both must serve the document.
        let html = "<p>x</p>";
        let resp = route_request("", html, None);
        assert_eq!(resp.status(), 200);
        assert_eq!(resp.body().as_ref(), html.as_bytes());
    }

    #[test]
    fn document_response_carries_content_type_and_nosniff() {
        let resp = route_request("/", "<p>x</p>", None);
        assert_eq!(
            resp.headers().get("Content-Type").unwrap(),
            "text/html; charset=utf-8"
        );
        assert_eq!(
            resp.headers().get("X-Content-Type-Options").unwrap(),
            "nosniff"
        );
    }

    #[test]
    fn document_response_carries_csp_header_scoped_to_viewer_scheme() {
        let resp = route_request("/", "<p>x</p>", None);
        let csp = resp
            .headers()
            .get("Content-Security-Policy")
            .expect("CSP header must be present")
            .to_str()
            .unwrap();
        assert!(csp.contains(SCHEME));
    }

    // ── CSP source-list scoping (AC-2) ───────────────────────────────────

    #[test]
    fn csp_covers_both_platform_scheme_forms() {
        let csp = build_csp(SCHEME);
        assert!(csp.contains(&format!("{SCHEME}:")));
        assert!(csp.contains(&format!("http://{SCHEME}.localhost")));
        assert!(csp.contains(&format!("https://{SCHEME}.localhost")));
    }

    #[test]
    fn csp_allows_inline_and_data_but_blocks_connect() {
        let csp = build_csp(SCHEME);
        assert!(csp.contains("'unsafe-inline'"));
        assert!(csp.contains("data:"));
        assert!(csp.contains("connect-src 'none'"));
    }

    #[test]
    fn csp_sources_contain_only_viewer_scheme_inline_and_data_tokens() {
        // AC-2: the source list must contain ONLY the viewer scheme forms,
        // inline allowances, and data: — no remote origin, no wildcard.
        let csp = build_csp(SCHEME);
        let allowed = [
            format!("{SCHEME}:"),
            format!("http://{SCHEME}.localhost"),
            format!("https://{SCHEME}.localhost"),
            "data:".to_string(),
            "'unsafe-inline'".to_string(),
            "'none'".to_string(),
        ];
        for directive in csp.split(';') {
            let mut tokens = directive.trim().split_whitespace();
            let _directive_name = tokens.next();
            for tok in tokens {
                assert!(
                    allowed.iter().any(|a| a == tok),
                    "unexpected CSP source {tok:?} in directive {directive:?}"
                );
            }
        }
    }

    // ── AC-3: resource requests through the resolver ─────────────────────

    #[test]
    fn traversal_resource_path_is_forbidden() {
        let resp = route_request("/../secret.png", "<html></html>", Some("/home/me/docs"));
        assert_eq!(resp.status(), 403);
    }

    #[test]
    fn absolute_resource_path_is_forbidden() {
        let resp = route_request("/etc/passwd.png", "<html></html>", Some("/home/me/docs"));
        assert_eq!(resp.status(), 403);
    }

    #[test]
    fn no_basedir_denies_any_resource_request() {
        let resp = route_request("/style.css", "<html></html>", None);
        assert_eq!(resp.status(), 403);
    }

    #[test]
    fn allowed_css_resource_under_basedir_serves_200_with_resolver_mime() {
        let dir = temp_dir("css");
        std::fs::write(dir.join("style.css"), b"body{color:red}").unwrap();
        let resp = route_request("/style.css", "<html></html>", Some(dir.to_str().unwrap()));
        assert_eq!(resp.status(), 200);
        assert_eq!(resp.headers().get("Content-Type").unwrap(), "text/css");
        assert_eq!(resp.body().as_ref(), b"body{color:red}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn allowed_image_resource_under_basedir_serves_200_with_resolver_mime() {
        let dir = temp_dir("img");
        std::fs::write(dir.join("a.png"), b"\x89PNG small").unwrap();
        let resp = route_request("/a.png", "<html></html>", Some(dir.to_str().unwrap()));
        assert_eq!(resp.status(), 200);
        assert_eq!(resp.headers().get("Content-Type").unwrap(), "image/png");
        assert_eq!(resp.body().as_ref(), b"\x89PNG small");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn disallowed_svg_resource_is_forbidden() {
        let dir = temp_dir("svg");
        std::fs::write(dir.join("diagram.svg"), b"<svg></svg>").unwrap();
        let resp = route_request("/diagram.svg", "<html></html>", Some(dir.to_str().unwrap()));
        assert_eq!(resp.status(), 403);
        let _ = std::fs::remove_dir_all(&dir);
    }

    // ── AC-4: navigation gate ─────────────────────────────────────────────

    #[test]
    fn in_scheme_navigation_is_allowed() {
        assert!(navigation_allowed("emterm-html-viewer://localhost/"));
        assert!(navigation_allowed(
            "emterm-html-viewer://localhost/img/a.png"
        ));
    }

    #[test]
    fn navigation_gate_accepts_webview2_workaround_forms() {
        assert!(navigation_allowed("http://emterm-html-viewer.localhost/"));
        assert!(navigation_allowed(
            "https://emterm-html-viewer.localhost/style.css"
        ));
        // Look-alike origins must still be rejected.
        assert!(!navigation_allowed(
            "http://emterm-html-viewer.localhost.evil.com/"
        ));
        assert!(!navigation_allowed("http://emterm-html-viewer/index.html"));
    }

    #[test]
    fn external_and_dangerous_navigation_is_denied_in_window() {
        // Pure predicate — must NOT spawn an OS browser (the side effect
        // lives in `handle_navigation`, exercised only at runtime).
        assert!(!navigation_allowed("https://example.com"));
        assert!(!navigation_allowed("http://example.com"));
        assert!(!navigation_allowed("file:///etc/passwd"));
        assert!(!navigation_allowed("javascript:alert(1)"));
        assert!(!navigation_allowed("data:text/html,<script>1</script>"));
    }

    // ── AC-4/AC-5: external-open decision seam (http(s) only) ────────────

    #[test]
    fn http_and_https_targets_are_marked_for_external_opening() {
        assert!(is_external_open_target("http://example.com"));
        assert!(is_external_open_target("https://example.com/path?x=1"));
    }

    #[test]
    fn non_http_targets_are_not_marked_for_external_opening() {
        assert!(!is_external_open_target("file:///etc/passwd"));
        assert!(!is_external_open_target("javascript:alert(1)"));
        assert!(!is_external_open_target("data:text/html,hi"));
        assert!(!is_external_open_target("mailto:test@example.com"));
        assert!(!is_external_open_target("emterm-html-viewer://localhost/"));
    }

    // ── AC-6: the child flag / window behavior are stable facts ──────────

    #[test]
    fn html_viewer_opens_maximized() {
        assert!(MAXIMIZED);
    }

    #[test]
    fn percent_decode_handles_spaces_and_passthrough() {
        assert_eq!(percent_decode("a%20b.png"), "a b.png");
        assert_eq!(percent_decode("plain.png"), "plain.png");
        assert_eq!(percent_decode("100%done"), "100%done");
    }

    // ── AC-1: payload read failures are reported, not panicked ───────────

    #[test]
    fn missing_payload_file_is_reported_as_err_not_panic() {
        let path = std::env::temp_dir().join("emterm-html-viewer-no-such-payload.json");
        let _ = std::fs::remove_file(&path);
        let result = run(path.to_str().unwrap());
        assert!(result.is_err());
    }

    #[test]
    fn unreadable_json_payload_is_reported_as_err_not_panic() {
        let path = std::env::temp_dir().join(format!(
            "emterm-html-viewer-bad-payload-{}-{}.json",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        std::fs::write(&path, b"not json").unwrap();
        let result = run(path.to_str().unwrap());
        assert!(result.is_err());
        // `run` deletes the payload on a successful read even when the JSON
        // itself is malformed only if it got past `read_to_string`; here the
        // JSON parse fails before deletion, so clean up defensively.
        let _ = std::fs::remove_file(&path);
    }
}
