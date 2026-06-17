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
