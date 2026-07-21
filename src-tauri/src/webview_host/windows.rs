//! Windows runtime for [`super::WebViewHost`]: winit window + WebView2 via
//! wry, driven by the standard Win32 message pump that winit runs.
//!
//! Unlike the Linux GTK path, there is no separate main-loop type to
//! manage: winit 0.31 removed the generic `user_event` payload (only
//! `EventLoopProxy::wake_up()` remains, carrying no data), so IPC bodies
//! from wry's worker thread reach the main thread via an `mpsc::channel`
//! paired with the wake-up call; [`WebViewApp::proxy_wake_up`] drains the
//! channel and runs the user's [`IpcHandler`](super::IpcHandler).

#![cfg(target_os = "windows")]

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, Sender};

use winit::application::ApplicationHandler;
use winit::event::{ElementState, KeyEvent, WindowEvent};
use winit::event_loop::{ActiveEventLoop, EventLoop, EventLoopProxy};
use winit::keyboard::{Key, NamedKey};
use winit::window::{Window, WindowAttributes, WindowId};
use wry::WebViewBuilder;
use wry::http::Request;

use super::{IpcHandler, NavigationHandler, RequestHandler, WebViewHost};

/// Run the configured child window on Windows. Blocks until the window
/// closes.
pub fn run(host: WebViewHost) -> Result<(), String> {
    let event_loop =
        EventLoop::new().map_err(|e| format!("webview_host: event loop build failed: {e}"))?;
    let proxy: EventLoopProxy = event_loop.create_proxy();
    let (ipc_tx, ipc_rx) = std::sync::mpsc::channel::<String>();

    let build_error: Rc<RefCell<Option<String>>> = Rc::new(RefCell::new(None));
    let app = WebViewApp {
        host: Some(host),
        proxy,
        ipc_tx,
        ipc_rx,
        window: None,
        webview: None,
        ipc_handler: None,
        close_on_esc_q: false,
        esc_guard: Arc::new(AtomicBool::new(false)),
        build_error: build_error.clone(),
    };
    event_loop
        .run_app(app)
        .map_err(|e| format!("webview_host: event loop returned error: {e}"))?;
    if let Some(err) = build_error.borrow().clone() {
        return Err(err);
    }
    Ok(())
}

struct WebViewApp {
    host: Option<WebViewHost>,
    proxy: EventLoopProxy,
    /// Cloned into the wry IPC callback (which may run on a worker thread
    /// on Windows) so a non-reserved IPC body reaches the main thread;
    /// paired with `proxy.wake_up()` since winit 0.31's `EventLoopProxy`
    /// only wakes the loop and carries no payload.
    ipc_tx: Sender<String>,
    /// Drained in [`Self::proxy_wake_up`].
    ipc_rx: Receiver<String>,
    window: Option<Rc<dyn Window>>,
    webview: Option<wry::WebView>,
    ipc_handler: Option<IpcHandler>,
    close_on_esc_q: bool,
    /// FR6 native ESC guard: while `true`, `KeyboardInput` does not exit on
    /// Esc / q / Q. Shared with the IPC handler closure (which may run off the
    /// main thread on Windows), which toggles it synchronously when it receives
    /// a reserved `__emterm_host:*` esc-guard message — before the body would
    /// ever be forwarded to the event loop — so the guard cannot lose the race
    /// against the first ESC keypress after a popup opens.
    esc_guard: Arc<AtomicBool>,
    build_error: Rc<RefCell<Option<String>>>,
}

impl ApplicationHandler for WebViewApp {
    fn can_create_surfaces(&mut self, event_loop: &dyn ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }
        let Some(host) = self.host.take() else {
            return;
        };

        let attrs = WindowAttributes::default()
            .with_title(host.title.clone())
            // `with_surface_size` is the restore size; `with_maximized`
            // (when the caller opts in) makes the window start maximized.
            .with_surface_size(winit::dpi::LogicalSize::new(
                host.initial_size.0,
                host.initial_size.1,
            ))
            .with_maximized(host.maximized)
            // FR3: attach the shared app icon to every wry child WebView
            // window (Markdown viewer / settings panel / data viewer) so
            // their title bars carry the eMterm glyph. The sibling
            // `linux.rs` is intentionally left untouched (see
            // `要件定義書.md` section 14.1 — Windows-only icon scope).
            .with_window_icon(crate::window_icon::app_icon());
        let window: Rc<dyn Window> = match event_loop.create_window(attrs) {
            Ok(w) => Rc::from(w),
            Err(e) => {
                *self.build_error.borrow_mut() =
                    Some(format!("webview_host: create_window failed: {e}"));
                event_loop.exit();
                return;
            }
        };

        let url = format!("{}://{}/{}", host.scheme, host.host, host.initial_url_path);
        let request_handler: RequestHandler = host.request_handler;
        let navigation_handler: NavigationHandler = host.navigation_handler;
        let new_window_handler = host.new_window_handler;
        let scheme = host.scheme.clone();
        let init_script = host.init_script;
        let has_ipc = host.ipc.is_some();
        let ipc_handler = host.ipc.map(|c| c.on_invoke);

        let ipc_tx = self.ipc_tx.clone();
        let ipc_proxy = self.proxy.clone();
        // FR6 native ESC guard shared between the IPC handler closure and the
        // `KeyboardInput` arm. The wry IPC callback may run off the main thread
        // on Windows, so an atomic is used instead of a plain bool.
        let esc_guard = Arc::new(AtomicBool::new(false));
        let mut builder = WebViewBuilder::new()
            .with_url(url)
            .with_custom_protocol(scheme, move |_id, request| request_handler(&request))
            .with_navigation_handler(move |uri| navigation_handler(&uri));

        if let Some(handler) = new_window_handler {
            // The popup is always denied in-WebView; the handler's only job
            // is any safe external-open side effect (see `NewWindowHandler`).
            // Per wry's docs the callback runs on a separate thread on
            // Windows to avoid a deadlock — `handler` must stay `Send + Sync`
            // (guaranteed by the `NewWindowHandler` type).
            builder = builder.with_new_window_req_handler(move |uri, _features| {
                handler(&uri);
                wry::NewWindowResponse::Deny
            });
        }

        if let Some(script) = init_script.as_deref() {
            builder = builder.with_initialization_script(script);
        }
        // Register the IPC handler whenever there is a user-level IPC bridge OR
        // the window opts into Esc / q / Q close: the latter needs `window.ipc`
        // to exist so the child frontend can post the reserved `__emterm_host:*`
        // esc-guard messages (FR6). Reserved messages are consumed here in the
        // IPC callback (toggling the guard synchronously) and never reach the
        // event loop or the user handler.
        if has_ipc || host.close_on_esc_q {
            let esc_guard = esc_guard.clone();
            builder = builder.with_ipc_handler(move |request: Request<String>| {
                let body = request.body();
                if let Some(msg) = super::parse_host_control(body) {
                    match msg {
                        super::HostControlMessage::EscGuardOn => {
                            esc_guard.store(true, Ordering::Relaxed)
                        }
                        super::HostControlMessage::EscGuardOff => {
                            esc_guard.store(false, Ordering::Relaxed)
                        }
                    }
                    return;
                }
                // Guard-only windows (close_on_esc_q without a user-level IPC
                // bridge) have no consumer for non-reserved bodies: drop them
                // here instead of sending a body the loop would only discard.
                if has_ipc {
                    // winit 0.31's `EventLoopProxy` carries no payload
                    // (`wake_up()` only), so the body travels over the
                    // channel and the wake-up merely signals "go check it".
                    let _ = ipc_tx.send(body.clone());
                    ipc_proxy.wake_up();
                }
            });
        }

        let webview = match builder.build(&window) {
            Ok(w) => w,
            Err(e) => {
                *self.build_error.borrow_mut() =
                    Some(format!("webview_host: webview build failed: {e}"));
                event_loop.exit();
                return;
            }
        };

        self.window = Some(window);
        self.webview = Some(webview);
        self.ipc_handler = ipc_handler;
        self.close_on_esc_q = host.close_on_esc_q;
        self.esc_guard = esc_guard;
    }

    fn window_event(
        &mut self,
        event_loop: &dyn ActiveEventLoop,
        _id: WindowId,
        event: WindowEvent,
    ) {
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::KeyboardInput {
                event:
                    KeyEvent {
                        state: ElementState::Pressed,
                        logical_key,
                        ..
                    },
                ..
            } if self.close_on_esc_q => {
                let close = matches!(
                    logical_key.as_ref(),
                    Key::Named(NamedKey::Escape) | Key::Character("q") | Key::Character("Q")
                );
                // FR6 native ESC guard: while a modal popup is open, do not
                // close the whole window on Esc / q / Q.
                if close && !self.esc_guard.load(Ordering::Relaxed) {
                    event_loop.exit();
                }
            }
            _ => {}
        }
    }

    fn proxy_wake_up(&mut self, _event_loop: &dyn ActiveEventLoop) {
        // Reserved `__emterm_host:*` messages are consumed in the IPC
        // callback (see `can_create_surfaces`) and never sent over
        // `ipc_tx`, so every body drained here is destined for the
        // user-level handler (FR6). winit 0.31's wake-up carries no
        // payload, so all bodies queued since the last drain are
        // processed in one pass (mirrors the old per-event `user_event`
        // delivery: each queued body still reaches the handler exactly
        // once, in order).
        while let Ok(body) = self.ipc_rx.try_recv() {
            let Some(handler) = self.ipc_handler.as_mut() else {
                continue;
            };
            let Some(script) = handler(body) else {
                continue;
            };
            if let Some(webview) = self.webview.as_ref() {
                if let Err(e) = webview.evaluate_script(&script) {
                    log::warn!("webview_host: reply eval failed: {e}");
                }
            }
        }
    }
}
