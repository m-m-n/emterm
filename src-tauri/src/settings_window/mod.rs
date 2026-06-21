//! Child `--settings` window.
//!
//! Runs the reused WebView settings panel in a separate child process via
//! the shared [`crate::webview_host`] runtime: GTK + WebKitGTK on Linux,
//! winit + WebView2 on Windows. The terminal's winit loop cannot drive
//! WebKitGTK, so the child owns its own window (and on Linux its own GTK
//! main loop); closing it never touches the terminal.
//!
//! Unlike the read-only viewers there is a bidirectional bridge: the
//! panel's Tauri `invoke()` calls arrive through wry's IPC channel as
//! `{id, cmd, args}` JSON messages, are dispatched to
//! [`commands::handle`], and the reply is delivered back by evaluating
//! `window.__EMTERM_SETTINGS_IPC__.resolve(id, ok, payload)`. After every
//! successful save the child prints [`SAVED_EVENT_LINE`] on stdout — the
//! parent terminal process watches the pipe and reloads + applies
//! `settings.json` live.

pub mod assets;
pub mod commands;

/// Stdout line the parent watches for. One line per persisted save.
pub const SAVED_EVENT_LINE: &str = "EMTERM_SETTINGS_SAVED";

/// Custom URI scheme the child serves its bundle from.
const SCHEME: &str = "emterm-settings";
/// Host used for in-bundle asset requests.
const HOST: &str = "localhost";

/// One parsed invoke message from the panel (`{id, cmd, args}`).
#[derive(Debug, Clone, PartialEq)]
pub struct InvokeMessage {
    pub id: u64,
    pub cmd: String,
    pub args: serde_json::Value,
}

/// Parse the raw IPC body. Returns `None` (with a warn) on malformed
/// input — a broken message must not take the window down.
pub fn parse_invoke(body: &str) -> Option<InvokeMessage> {
    let v: serde_json::Value = match serde_json::from_str(body) {
        Ok(v) => v,
        Err(e) => {
            log::warn!("settings window: malformed ipc message: {e}");
            return None;
        }
    };
    let id = v.get("id").and_then(serde_json::Value::as_u64)?;
    let cmd = v
        .get("cmd")
        .and_then(serde_json::Value::as_str)?
        .to_string();
    let args = v.get("args").cloned().unwrap_or(serde_json::Value::Null);
    Some(InvokeMessage { id, cmd, args })
}

/// Build the reply script evaluated in the WebView for one handled call.
///
/// The payload is interpolated as a JSON literal; U+2028 / U+2029 are
/// re-escaped because they were JS line terminators before ES2019 (same
/// hardening as the Markdown viewer's payload injection).
pub fn reply_script(id: u64, result: &Result<serde_json::Value, String>) -> String {
    let (ok, payload) = match result {
        Ok(v) => (true, v.clone()),
        Err(e) => (false, serde_json::Value::String(e.clone())),
    };
    let json = serde_json::to_string(&payload)
        .unwrap_or_else(|_| "null".to_string())
        .replace('\u{2028}', "\\u2028")
        .replace('\u{2029}', "\\u2029");
    format!("window.__EMTERM_SETTINGS_IPC__.resolve({id}, {ok}, {json});")
}

/// Run the child settings window. Blocks until the window closes.
#[cfg(any(target_os = "linux", target_os = "windows"))]
pub fn run() -> Result<(), String> {
    if !assets::is_embedded() {
        return Err("settings window: bundle not embedded (run `bun run build:settings`)".into());
    }

    build_host().run()
}

/// Build the settings [`WebViewHost`] config (without running it).
///
/// Split out from [`run`] so the maximize-on-launch decision (FR1) is a
/// deterministic, unit-testable fact: the returned config carries
/// `maximized: true`. All handlers here are self-contained `'static`
/// closures, so building the config has no side effects.
#[cfg(any(target_os = "linux", target_os = "windows"))]
fn build_host() -> crate::webview_host::WebViewHost {
    use std::io::Write as _;

    use crate::webview_host::{IpcConfig, WebViewHost};

    WebViewHost {
        scheme: SCHEME.to_string(),
        host: HOST.to_string(),
        title: window_title(),
        initial_size: (1080.0, 760.0),
        initial_url_path: assets::INDEX_PATH.to_string(),
        init_script: None,
        request_handler: Box::new(|request| handle_request(request)),
        navigation_handler: Box::new(|uri| handle_navigation(uri)),
        ipc: Some(IpcConfig {
            on_invoke: Box::new(|body| {
                let msg = parse_invoke(&body)?;
                let outcome = commands::handle(&msg.cmd, &msg.args);
                if let Err(e) = &outcome.result {
                    log::warn!("settings window: {} failed: {e}", msg.cmd);
                }
                let script = reply_script(msg.id, &outcome.result);
                if outcome.saved {
                    // Line-oriented save signal for the parent's pipe reader.
                    let mut out = std::io::stdout().lock();
                    let _ = writeln!(out, "{SAVED_EVENT_LINE}");
                    let _ = out.flush();
                }
                Some(script)
            }),
        }),
        // The panel is full of text inputs and its own Esc-closing
        // dialogs, so Esc/Q must not exit the window.
        close_on_esc_q: false,
        // FR1: open maximized; `initial_size` above is the restore size.
        maximized: true,
    }
}

/// Window title, localized from the persisted language (the child has no
/// other locale source before the bundle boots).
#[cfg(any(target_os = "linux", target_os = "windows"))]
fn window_title() -> String {
    let settings = crate::settings::Settings::load_or_default();
    match crate::i18n::resolve(settings.language) {
        crate::i18n::Locale::Ja => "eMterm 設定".to_string(),
        crate::i18n::Locale::En => "eMterm Settings".to_string(),
    }
}

/// Custom-scheme request router: serves in-bundle assets only.
#[cfg(any(target_os = "linux", target_os = "windows"))]
fn handle_request(
    request: &wry::http::Request<Vec<u8>>,
) -> wry::http::Response<std::borrow::Cow<'static, [u8]>> {
    use std::borrow::Cow;
    use wry::http::Response;

    match assets::asset(request.uri().path()) {
        Some((bytes, content_type)) => Response::builder()
            .status(200)
            .header("Content-Type", content_type)
            // Prevent MIME sniffing into the privileged settings origin.
            .header("X-Content-Type-Options", "nosniff")
            .body(Cow::Borrowed(bytes))
            .unwrap_or_else(|_| not_found()),
        None => not_found(),
    }
}

#[cfg(any(target_os = "linux", target_os = "windows"))]
fn not_found() -> wry::http::Response<std::borrow::Cow<'static, [u8]>> {
    wry::http::Response::builder()
        .status(404)
        .body(std::borrow::Cow::Borrowed(&b"not found"[..]))
        .expect("static 404 response")
}

/// In-window navigation is allowed only inside the bundle origin.
///
/// WebView2 on Windows cannot register non-standard URI schemes, so wry
/// rewrites `emterm-settings://localhost/...` to
/// `http://emterm-settings.localhost/...` before navigating (see wry
/// `apply_uri_work_around`). The rewritten form arrives at this gate, so
/// we accept it alongside the original scheme.
pub fn navigation_allowed(uri: &str) -> bool {
    uri.starts_with("emterm-settings://")
        || uri.starts_with("http://emterm-settings.localhost/")
        || uri.starts_with("https://emterm-settings.localhost/")
}

/// Navigation handler: in-bundle proceeds; anything else is denied
/// in-window and safe external schemes open in the OS browser.
#[cfg(any(target_os = "linux", target_os = "windows"))]
fn handle_navigation(uri: &str) -> bool {
    if navigation_allowed(uri) {
        return true;
    }
    crate::links::open_safe_uri(uri);
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parse_invoke_reads_id_cmd_args() {
        let msg = parse_invoke(r#"{"id": 7, "cmd": "load_settings", "args": {"a": 1}}"#).unwrap();
        assert_eq!(msg.id, 7);
        assert_eq!(msg.cmd, "load_settings");
        assert_eq!(msg.args, json!({"a": 1}));
    }

    #[test]
    fn parse_invoke_tolerates_missing_args() {
        let msg = parse_invoke(r#"{"id": 1, "cmd": "x"}"#).unwrap();
        assert_eq!(msg.args, serde_json::Value::Null);
    }

    #[test]
    fn parse_invoke_rejects_malformed_input() {
        assert!(parse_invoke("not json").is_none());
        assert!(parse_invoke(r#"{"cmd": "x"}"#).is_none());
        assert!(parse_invoke(r#"{"id": 1}"#).is_none());
    }

    #[test]
    fn reply_script_encodes_ok_and_err() {
        let ok = reply_script(3, &Ok(json!({"k": "v"})));
        assert_eq!(
            ok,
            r#"window.__EMTERM_SETTINGS_IPC__.resolve(3, true, {"k":"v"});"#
        );
        let err = reply_script(4, &Err("boom".to_string()));
        assert_eq!(
            err,
            r#"window.__EMTERM_SETTINGS_IPC__.resolve(4, false, "boom");"#
        );
    }

    #[test]
    fn reply_script_escapes_js_line_separators() {
        let s = reply_script(1, &Ok(json!("a\u{2028}b\u{2029}c")));
        assert!(!s.contains('\u{2028}'));
        assert!(!s.contains('\u{2029}'));
        assert!(s.contains("\\u2028"));
        assert!(s.contains("\\u2029"));
    }

    #[test]
    fn navigation_gate_only_allows_bundle_origin() {
        assert!(navigation_allowed("emterm-settings://localhost/index.html"));
        assert!(!navigation_allowed("https://example.com"));
        assert!(!navigation_allowed("file:///etc/passwd"));
        assert!(!navigation_allowed("javascript:alert(1)"));
    }

    #[test]
    fn navigation_gate_accepts_webview2_workaround_form() {
        // WebView2 rewrites `emterm-settings://localhost/...` to
        // `http(s)://emterm-settings.localhost/...`; the rewritten form
        // must still count as in-bundle so wry's NavigationStarting
        // callback proceeds instead of being routed to the OS browser.
        assert!(navigation_allowed(
            "http://emterm-settings.localhost/index.html"
        ));
        assert!(navigation_allowed(
            "https://emterm-settings.localhost/assets/app.js"
        ));
        // Look-alike origins must still be rejected.
        assert!(!navigation_allowed(
            "http://emterm-settings.localhost.evil.com/"
        ));
        assert!(!navigation_allowed("http://emterm-settings/index.html"));
    }

    // TS-1: the settings host config carries the maximize-on-launch flag
    // (FR1). `initial_size` is preserved as the restore size.
    #[cfg(any(target_os = "linux", target_os = "windows"))]
    #[test]
    fn settings_host_opens_maximized_with_restore_size() {
        let host = build_host();
        assert!(host.maximized);
        assert_eq!(host.initial_size, (1080.0, 760.0));
    }
}
