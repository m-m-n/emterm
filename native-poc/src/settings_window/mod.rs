//! Child `--settings` window (Linux GTK / WebKitGTK via wry).
//!
//! Runs the reused WebView settings panel in a separate child process,
//! following the Markdown viewer's architecture (`viewer::window`): the
//! terminal's winit loop cannot drive WebKitGTK, so the child owns its own
//! GTK window and main loop, and closing it never touches the terminal.
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
#[cfg(any(target_os = "linux", target_os = "windows"))]
const SCHEME: &str = "emterm-settings";
/// Host used for in-bundle asset requests.
#[cfg(any(target_os = "linux", target_os = "windows"))]
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
#[cfg(target_os = "linux")]
pub fn run() -> Result<(), String> {
    use std::io::Write as _;

    use gtk::prelude::*;
    use gtk::{Window, WindowType};
    use wry::http::Request;
    use wry::{WebViewBuilder, WebViewBuilderExtUnix};

    if !assets::is_embedded() {
        return Err("settings window: bundle not embedded (run `bun run build:settings`)".into());
    }

    gtk::init().map_err(|e| format!("settings window: gtk init failed: {e}"))?;

    let window = Window::new(WindowType::Toplevel);
    window.set_title(&window_title());
    window.set_default_size(1080, 760);

    // invoke() calls land here (any wry worker thread) and are drained on
    // the GTK main loop below, where the WebView handle lives.
    let (tx, rx) = std::sync::mpsc::channel::<String>();

    let builder = WebViewBuilder::new()
        .with_url(format!("{SCHEME}://{HOST}/{}", assets::INDEX_PATH))
        .with_custom_protocol(SCHEME.to_string(), move |_id, request| {
            handle_request(&request)
        })
        .with_ipc_handler(move |request: Request<String>| {
            let _ = tx.send(request.body().clone());
        })
        // Deny in-window navigation away from the bundle; route safe
        // external URIs to the OS (same gate as the Markdown viewer).
        .with_navigation_handler(|uri| handle_navigation(&uri));

    let webview = builder
        .build_gtk(&window)
        .map_err(|e| format!("settings window: webview build failed: {e}"))?;

    let running = std::rc::Rc::new(std::cell::Cell::new(true));
    {
        let running = running.clone();
        window.connect_delete_event(move |_, _| {
            running.set(false);
            gtk::glib::Propagation::Proceed
        });
    }
    // No Esc/q shortcuts here (unlike the read-only viewers): the panel is
    // full of text inputs and its own Esc-closing dialogs.

    window.show_all();

    // Child-owned GTK main loop; after each iteration drain the pending
    // invoke calls and reply on the main thread.
    while running.get() {
        gtk::main_iteration_do(true);
        while let Ok(body) = rx.try_recv() {
            let Some(msg) = parse_invoke(&body) else {
                continue;
            };
            let outcome = commands::handle(&msg.cmd, &msg.args);
            if let Err(e) = &outcome.result {
                log::warn!("settings window: {} failed: {e}", msg.cmd);
            }
            let script = reply_script(msg.id, &outcome.result);
            if let Err(e) = webview.evaluate_script(&script) {
                log::warn!("settings window: reply eval failed: {e}");
            }
            if outcome.saved {
                // Line-oriented save signal for the parent's pipe reader.
                let mut out = std::io::stdout().lock();
                let _ = writeln!(out, "{SAVED_EVENT_LINE}");
                let _ = out.flush();
            }
        }
    }
    Ok(())
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

/// Run the child settings window on Windows (wry + WebView2 hosted in a
/// winit window). The Linux path owns its own GTK main loop because
/// WebKitGTK requires it; on Windows wry's WebView2 backend is driven by
/// the standard Win32 message pump that winit already runs, so the
/// implementation is a single `EventLoop::run_app` over a user-event
/// stream carrying IPC bodies from wry's worker thread back to the main
/// thread.
#[cfg(target_os = "windows")]
pub fn run() -> Result<(), String> {
    use std::cell::RefCell;
    use std::io::Write as _;
    use std::rc::Rc;

    use winit::application::ApplicationHandler;
    use winit::event::WindowEvent;
    use winit::event_loop::{ActiveEventLoop, EventLoop, EventLoopProxy};
    use winit::window::{Window, WindowAttributes, WindowId};
    use wry::http::Request;
    use wry::WebViewBuilder;

    if !assets::is_embedded() {
        return Err("settings window: bundle not embedded (run `bun run build:settings`)".into());
    }

    let event_loop = EventLoop::<String>::with_user_event()
        .build()
        .map_err(|e| format!("settings window: event loop build failed: {e}"))?;
    let proxy: EventLoopProxy<String> = event_loop.create_proxy();

    struct SettingsApp {
        proxy: EventLoopProxy<String>,
        window: Option<Rc<Window>>,
        webview: Option<wry::WebView>,
        build_error: Rc<RefCell<Option<String>>>,
    }

    impl ApplicationHandler<String> for SettingsApp {
        fn resumed(&mut self, event_loop: &ActiveEventLoop) {
            if self.window.is_some() {
                return;
            }
            let attrs = WindowAttributes::default()
                .with_title(window_title())
                .with_inner_size(winit::dpi::LogicalSize::new(1080.0, 760.0));
            let window = match event_loop.create_window(attrs) {
                Ok(w) => Rc::new(w),
                Err(e) => {
                    *self.build_error.borrow_mut() =
                        Some(format!("settings window: create_window failed: {e}"));
                    event_loop.exit();
                    return;
                }
            };
            let ipc_proxy = self.proxy.clone();
            let builder = WebViewBuilder::new()
                .with_url(format!("{SCHEME}://{HOST}/{}", assets::INDEX_PATH))
                .with_custom_protocol(SCHEME.to_string(), move |_id, request| {
                    handle_request(&request)
                })
                .with_ipc_handler(move |request: Request<String>| {
                    let _ = ipc_proxy.send_event(request.body().clone());
                })
                .with_navigation_handler(|uri| handle_navigation(&uri));
            let webview = match builder.build(window.as_ref()) {
                Ok(w) => w,
                Err(e) => {
                    *self.build_error.borrow_mut() =
                        Some(format!("settings window: webview build failed: {e}"));
                    event_loop.exit();
                    return;
                }
            };
            self.window = Some(window);
            self.webview = Some(webview);
        }

        fn window_event(
            &mut self,
            event_loop: &ActiveEventLoop,
            _id: WindowId,
            event: WindowEvent,
        ) {
            if matches!(event, WindowEvent::CloseRequested) {
                event_loop.exit();
            }
        }

        fn user_event(&mut self, _event_loop: &ActiveEventLoop, body: String) {
            let Some(msg) = parse_invoke(&body) else {
                return;
            };
            let outcome = commands::handle(&msg.cmd, &msg.args);
            if let Err(e) = &outcome.result {
                log::warn!("settings window: {} failed: {e}", msg.cmd);
            }
            let script = reply_script(msg.id, &outcome.result);
            if let Some(webview) = self.webview.as_ref() {
                if let Err(e) = webview.evaluate_script(&script) {
                    log::warn!("settings window: reply eval failed: {e}");
                }
            }
            if outcome.saved {
                let mut out = std::io::stdout().lock();
                let _ = writeln!(out, "{SAVED_EVENT_LINE}");
                let _ = out.flush();
            }
        }
    }

    let build_error: Rc<RefCell<Option<String>>> = Rc::new(RefCell::new(None));
    let mut app = SettingsApp {
        proxy,
        window: None,
        webview: None,
        build_error: build_error.clone(),
    };
    event_loop
        .run_app(&mut app)
        .map_err(|e| format!("settings window: event loop returned error: {e}"))?;
    if let Some(err) = build_error.borrow().clone() {
        return Err(err);
    }
    Ok(())
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
}
