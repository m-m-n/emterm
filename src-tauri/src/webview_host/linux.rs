//! Linux runtime for [`super::WebViewHost`]: GTK window + WebKitGTK via wry,
//! driven by a child-owned GTK main loop.
//!
//! The child owns its own `gtk::Window` and main loop because WebKitGTK
//! requires a GTK container plus the GTK event loop, which the terminal's
//! winit loop cannot drive. IPC bodies arrive on a wry worker thread,
//! are forwarded over a `std::sync::mpsc` channel, and are drained on
//! the main loop where the WebView handle lives.

#![cfg(target_os = "linux")]

use gtk::prelude::*;
use gtk::{Window, WindowType};
use wry::http::Request;
use wry::{WebViewBuilder, WebViewBuilderExtUnix};

use super::WebViewHost;

/// Run the configured child window on Linux. Blocks until the window
/// closes.
pub fn run(host: WebViewHost) -> Result<(), String> {
    gtk::init().map_err(|e| format!("webview_host: gtk init failed: {e}"))?;

    // FR5: GTK derives the X11 `WM_CLASS` and the Wayland `app_id` from
    // the program identity, not from a per-window call. Set both to the
    // canonical identifier once (after `gtk::init`, before any window is
    // created) so the settings + Markdown windows report the same
    // `emterm` as the winit windows and group under one dock icon.
    gtk::glib::set_prgname(Some(crate::APP_WM_ID));
    gtk::gdk::set_program_class(crate::APP_WM_ID);

    let window = Window::new(WindowType::Toplevel);
    window.set_title(&host.title);
    // `set_default_size` is the restore size the window returns to when
    // un-maximized; `maximize()` (called below, before `show_all`) makes
    // it start maximized when the caller opts in.
    window.set_default_size(host.initial_size.0 as i32, host.initial_size.1 as i32);
    if host.maximized {
        // Request maximize before the window is mapped so it appears
        // maximized from the first frame (no resize flicker). The
        // default size above remains the un-maximize restore size.
        window.maximize();
    }

    // IPC bodies arrive on wry's worker thread; we forward them to the
    // GTK main loop via a channel and dispatch the user's IpcHandler
    // from there (so the handler runs on the main thread, same thread as
    // the WebView handle used for `evaluate_script`).
    let (ipc_tx, ipc_rx) = std::sync::mpsc::channel::<String>();
    let has_ipc = host.ipc.is_some();
    let mut ipc_handler = host.ipc.map(|c| c.on_invoke);

    // FR6 native ESC guard: while `true`, the key_press handler below does not
    // close the window on Esc / q / Q. Toggled synchronously in the IPC callback
    // (WebKitGTK dispatches script messages on the GTK main thread, so mutating
    // this shared cell there is safe). Toggling in the callback — before the
    // body would ever be enqueued — keeps the guard from losing the race
    // against the first ESC keypress after a popup opens.
    let esc_guard = std::rc::Rc::new(std::cell::Cell::new(false));

    let scheme = host.scheme.clone();
    let request_handler = host.request_handler;
    let navigation_handler = host.navigation_handler;
    let new_window_handler = host.new_window_handler;
    let init_script = host.init_script;

    let url = format!("{}://{}/{}", host.scheme, host.host, host.initial_url_path);

    let mut builder = WebViewBuilder::new()
        .with_url(url)
        .with_custom_protocol(scheme, move |_id, request| request_handler(&request))
        .with_navigation_handler(move |uri| navigation_handler(&uri));

    if let Some(handler) = new_window_handler {
        // The popup is always denied in-WebView; the handler's only job is
        // any safe external-open side effect (see `NewWindowHandler`).
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
    // esc-guard messages (FR6). Reserved messages are consumed here in the IPC
    // callback and never enter the channel, so they can neither reach the user
    // handler nor contribute to unbounded queue growth.
    if has_ipc || host.close_on_esc_q {
        let esc_guard = esc_guard.clone();
        builder = builder.with_ipc_handler(move |request: Request<String>| {
            let body = request.body();
            if let Some(msg) = super::parse_host_control(body) {
                match msg {
                    super::HostControlMessage::EscGuardOn => esc_guard.set(true),
                    super::HostControlMessage::EscGuardOff => esc_guard.set(false),
                }
                return;
            }
            // Guard-only windows (close_on_esc_q without a user-level IPC
            // bridge) have no consumer for non-reserved bodies: drop them
            // here instead of growing the channel with messages the drain
            // would only discard.
            if has_ipc {
                let _ = ipc_tx.send(body.clone());
            }
        });
    }

    let webview = builder
        .build_gtk(&window)
        .map_err(|e| format!("webview_host: webview build failed: {e}"))?;

    let running = std::rc::Rc::new(std::cell::Cell::new(true));
    {
        let running = running.clone();
        window.connect_delete_event(move |_, _| {
            running.set(false);
            gtk::glib::Propagation::Proceed
        });
    }
    if host.close_on_esc_q {
        let running = running.clone();
        let esc_guard = esc_guard.clone();
        window.connect_key_press_event(move |_, ev| {
            let key = ev.keyval();
            if !esc_guard.get()
                && (key == gtk::gdk::keys::constants::Escape
                    || key == gtk::gdk::keys::constants::q
                    || key == gtk::gdk::keys::constants::Q)
            {
                running.set(false);
            }
            gtk::glib::Propagation::Proceed
        });
    }

    window.show_all();

    // Drain IPC bodies destined for the user-level handler. Reserved
    // `__emterm_host:*` messages are consumed synchronously in the IPC
    // callback above and never enter this channel. Cap the drain at 64
    // bodies per GTK iteration so an IPC flood cannot starve UI event
    // processing — but re-arm when the cap was hit: `main_iteration_do(true)`
    // blocks and an mpsc send does not wake the GTK context, so leftover
    // bodies would otherwise be stranded until an unrelated GTK event.
    let mut block = true;
    while running.get() {
        gtk::main_iteration_do(block);
        let mut drained = 0;
        while drained < 64 {
            let Ok(body) = ipc_rx.try_recv() else {
                break;
            };
            drained += 1;
            if let Some(handler) = ipc_handler.as_mut() {
                if let Some(script) = handler(body) {
                    if let Err(e) = webview.evaluate_script(&script) {
                        log::warn!("webview_host: reply eval failed: {e}");
                    }
                }
            }
        }
        block = drained < 64;
    }
    Ok(())
}
