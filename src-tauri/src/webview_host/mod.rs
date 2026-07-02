//! Shared child-process WebView host (Linux GTK+WebKitGTK / Windows winit+WebView2).
//!
//! Both the settings panel (`settings_window`) and the Markdown viewer
//! (`viewer::window`) run as separate child processes that own a single
//! `wry::WebView` window. The terminal's winit loop cannot drive
//! WebKitGTK (it needs a GTK main loop), and on Windows wry's WebView2
//! backend is driven by the standard Win32 message pump that winit
//! already runs. The OS-specific wiring is the same for both child
//! windows; only the per-window config (URI scheme, asset handler,
//! optional IPC bridge, optional init script) differs.
//!
//! [`WebViewHost`] is the per-window config; [`WebViewHost::run`]
//! dispatches to the OS-specific runtime ([`linux::run`] / [`windows::run`])
//! and blocks until the window closes.

use std::borrow::Cow;

use wry::http::{Request, Response};

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "windows")]
mod windows;

/// Closure type for the custom URI scheme responder. Called on a wry
/// worker thread; serves in-bundle assets and (optionally) basedir-confined
/// local resources.
pub type RequestHandler =
    Box<dyn Fn(&Request<Vec<u8>>) -> Response<Cow<'static, [u8]>> + Send + Sync + 'static>;

/// Closure type for the navigation gate. Returns `true` to allow in-window
/// navigation, `false` to deny (with any safe external scheme handling
/// performed as a side effect inside the closure).
pub type NavigationHandler = Box<dyn Fn(&str) -> bool + Send + Sync + 'static>;

/// IPC dispatcher closure type. Called on the main thread once per
/// `invoke()` body received from the WebView. Returns an optional
/// `evaluate_script` body to send back as the reply.
pub type IpcHandler = Box<dyn FnMut(String) -> Option<String> + 'static>;

/// Reserved host-control IPC messages sent by child WebView frontends
/// (see `web-shared/markdown/mermaid-popup.ts`). These are consumed by the
/// host layer and never forwarded to a user-level [`IpcHandler`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum HostControlMessage {
    /// Suppress the native Esc / q / Q window-close while a modal popup is
    /// open, so a single ESC closes only the popup and not the whole window.
    EscGuardOn,
    /// Re-enable the native Esc / q / Q window-close.
    EscGuardOff,
}

/// Parse a reserved `__emterm_host:*` control message. Returns `None` for any
/// body that is not an exact reserved message (which the host then forwards to
/// the user-level [`IpcHandler`]).
pub(crate) fn parse_host_control(body: &str) -> Option<HostControlMessage> {
    match body {
        "__emterm_host:esc-guard:on" => Some(HostControlMessage::EscGuardOn),
        "__emterm_host:esc-guard:off" => Some(HostControlMessage::EscGuardOff),
        _ => None,
    }
}

/// Optional bidirectional IPC bridge config. When `Some`, the host
/// installs a `with_ipc_handler` that forwards each body to the main
/// thread; the main thread invokes [`IpcHandler`] and evaluates any
/// returned reply script in the WebView.
pub struct IpcConfig {
    pub on_invoke: IpcHandler,
}

/// One child-window WebView host config. Construct from the caller
/// (settings/viewer) and call [`run`](Self::run) to block until the
/// window closes.
pub struct WebViewHost {
    /// Custom URI scheme served by [`RequestHandler`]
    /// (e.g. `"emterm-settings"`, `"emterm-viewer"`).
    pub scheme: String,
    /// Host segment of the in-bundle URL (e.g. `"localhost"`).
    pub host: String,
    /// OS window title.
    pub title: String,
    /// Default window size in logical pixels.
    pub initial_size: (f64, f64),
    /// In-bundle path of the entry document (e.g. `"index.html"`).
    /// Combined with `scheme`/`host` into the initial `with_url`.
    pub initial_url_path: String,
    /// Optional init script run before the bundle entry. Used by the
    /// viewer to inject `window.__EMTERM_VIEWER_PAYLOAD__`; the settings
    /// panel passes `None`.
    pub init_script: Option<String>,
    /// Custom-scheme responder. Both windows route in-bundle assets here;
    /// the viewer also resolves basedir-confined image requests inside
    /// its handler.
    pub request_handler: RequestHandler,
    /// Navigation gate (FR7-equivalent). Both windows deny external
    /// navigation in-window and route safe URIs to the OS handler from
    /// inside the closure.
    pub navigation_handler: NavigationHandler,
    /// Optional bidirectional IPC bridge. `Some` for the settings panel
    /// (`__EMTERM_SETTINGS_IPC__.resolve(...)`); `None` for the
    /// read-only viewer.
    pub ipc: Option<IpcConfig>,
    /// True when `Esc` / `q` / `Q` should close the window. Viewer-style
    /// read-only windows opt in; the settings panel (full of text
    /// inputs) opts out.
    pub close_on_esc_q: bool,
    /// Opt-in: start the window maximized. The settings panel and the
    /// Markdown viewer set this; `initial_size` is kept as the restore
    /// size the window returns to when un-maximized. Defaults to `false`
    /// (open at `initial_size`), so the field is additive for any caller
    /// that does not want maximize-on-launch.
    pub maximized: bool,
}

impl WebViewHost {
    /// Run the configured child window. Blocks until the window closes,
    /// then returns. Any setup failure logs and returns an `Err` so the
    /// child can exit non-zero.
    pub fn run(self) -> Result<(), String> {
        #[cfg(target_os = "linux")]
        {
            linux::run(self)
        }
        #[cfg(target_os = "windows")]
        {
            windows::run(self)
        }
        #[cfg(not(any(target_os = "linux", target_os = "windows")))]
        {
            let _ = self;
            Err("webview_host: unsupported platform".into())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{HostControlMessage, parse_host_control};

    #[test]
    fn parses_esc_guard_on() {
        assert_eq!(
            parse_host_control("__emterm_host:esc-guard:on"),
            Some(HostControlMessage::EscGuardOn)
        );
    }

    #[test]
    fn parses_esc_guard_off() {
        assert_eq!(
            parse_host_control("__emterm_host:esc-guard:off"),
            Some(HostControlMessage::EscGuardOff)
        );
    }

    #[test]
    fn rejects_arbitrary_body() {
        assert_eq!(parse_host_control(""), None);
        assert_eq!(parse_host_control("hello"), None);
        assert_eq!(parse_host_control("{\"id\":1,\"cmd\":\"save\"}"), None);
    }

    #[test]
    fn rejects_prefixed_but_wrong() {
        // Right prefix, wrong / partial suffix must not parse.
        assert_eq!(parse_host_control("__emterm_host:esc-guard:"), None);
        assert_eq!(parse_host_control("__emterm_host:esc-guard:onx"), None);
        assert_eq!(parse_host_control("__emterm_host:esc-guard:on "), None);
        assert_eq!(parse_host_control("__emterm_host:other"), None);
        // Leading whitespace must not parse (exact match only).
        assert_eq!(parse_host_control(" __emterm_host:esc-guard:on"), None);
    }

    /// Cross-language contract drift guard (same spirit as
    /// `ui::dialog::tests` for the design tokens): the reserved message
    /// literals accepted by [`parse_host_control`] must byte-for-byte match
    /// the constants the TypeScript frontend posts. A rename on either side
    /// would otherwise keep both sides' local tests green while silently
    /// disabling the native ESC guard.
    #[test]
    fn ts_frontend_posts_the_exact_reserved_literals() {
        let ts_source = std::fs::read_to_string(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/web-shared/markdown/mermaid-popup.ts"
        ))
        .expect("mermaid-popup.ts must exist (esc-guard producer)");

        for literal in ["__emterm_host:esc-guard:on", "__emterm_host:esc-guard:off"] {
            assert!(
                ts_source.contains(&format!("\"{literal}\"")),
                "mermaid-popup.ts no longer contains the reserved literal {literal:?}; \
                 the Rust parser and the TS producer have drifted apart"
            );
            assert!(
                parse_host_control(literal).is_some(),
                "parse_host_control must accept {literal:?}"
            );
        }
    }
}
