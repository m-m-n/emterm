//! Windows runtime for [`super::WebViewHost`]: winit window + WebView2 via
//! wry, driven by the standard Win32 message pump that winit runs.
//!
//! Unlike the Linux GTK path, there is no separate main-loop type to
//! manage: winit's `EventLoop::with_user_event` carries IPC bodies from
//! wry's worker thread back to the main thread, where the user's
//! [`IpcHandler`](super::IpcHandler) runs inside `user_event`.

#![cfg(target_os = "windows")]

use std::cell::RefCell;
use std::rc::Rc;

use winit::application::ApplicationHandler;
use winit::event::{ElementState, KeyEvent, WindowEvent};
use winit::event_loop::{ActiveEventLoop, EventLoop, EventLoopProxy};
use winit::keyboard::{Key, NamedKey};
use winit::window::{Window, WindowAttributes, WindowId};
use wry::http::Request;
use wry::WebViewBuilder;

use super::{IpcHandler, NavigationHandler, RequestHandler, WebViewHost};

/// Run the configured child window on Windows. Blocks until the window
/// closes.
pub fn run(host: WebViewHost) -> Result<(), String> {
    let event_loop = EventLoop::<String>::with_user_event()
        .build()
        .map_err(|e| format!("webview_host: event loop build failed: {e}"))?;
    let proxy: EventLoopProxy<String> = event_loop.create_proxy();

    let build_error: Rc<RefCell<Option<String>>> = Rc::new(RefCell::new(None));
    let mut app = WebViewApp {
        host: Some(host),
        proxy,
        window: None,
        webview: None,
        ipc_handler: None,
        close_on_esc_q: false,
        build_error: build_error.clone(),
    };
    event_loop
        .run_app(&mut app)
        .map_err(|e| format!("webview_host: event loop returned error: {e}"))?;
    if let Some(err) = build_error.borrow().clone() {
        return Err(err);
    }
    Ok(())
}

struct WebViewApp {
    host: Option<WebViewHost>,
    proxy: EventLoopProxy<String>,
    window: Option<Rc<Window>>,
    webview: Option<wry::WebView>,
    ipc_handler: Option<IpcHandler>,
    close_on_esc_q: bool,
    build_error: Rc<RefCell<Option<String>>>,
}

impl ApplicationHandler<String> for WebViewApp {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }
        let Some(host) = self.host.take() else {
            return;
        };

        let attrs = WindowAttributes::default()
            .with_title(host.title.clone())
            .with_inner_size(winit::dpi::LogicalSize::new(
                host.initial_size.0,
                host.initial_size.1,
            ));
        let window = match event_loop.create_window(attrs) {
            Ok(w) => Rc::new(w),
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
        let scheme = host.scheme.clone();
        let init_script = host.init_script;
        let has_ipc = host.ipc.is_some();
        let ipc_handler = host.ipc.map(|c| c.on_invoke);

        let ipc_proxy = self.proxy.clone();
        let mut builder = WebViewBuilder::new()
            .with_url(url)
            .with_custom_protocol(scheme, move |_id, request| request_handler(&request))
            .with_navigation_handler(move |uri| navigation_handler(&uri));

        if let Some(script) = init_script.as_deref() {
            builder = builder.with_initialization_script(script);
        }
        if has_ipc {
            builder = builder.with_ipc_handler(move |request: Request<String>| {
                let _ = ipc_proxy.send_event(request.body().clone());
            });
        }

        let webview = match builder.build(window.as_ref()) {
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
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
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
                if close {
                    event_loop.exit();
                }
            }
            _ => {}
        }
    }

    fn user_event(&mut self, _event_loop: &ActiveEventLoop, body: String) {
        let Some(handler) = self.ipc_handler.as_mut() else {
            return;
        };
        let Some(script) = handler(body) else {
            return;
        };
        if let Some(webview) = self.webview.as_ref() {
            if let Err(e) = webview.evaluate_script(&script) {
                log::warn!("webview_host: reply eval failed: {e}");
            }
        }
    }
}
