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

    let window = Window::new(WindowType::Toplevel);
    window.set_title(&host.title);
    window.set_default_size(host.initial_size.0 as i32, host.initial_size.1 as i32);

    // IPC bodies arrive on wry's worker thread; we forward them to the
    // GTK main loop via a channel and dispatch the user's IpcHandler
    // from there (so the handler runs on the main thread, same thread as
    // the WebView handle used for `evaluate_script`).
    let (ipc_tx, ipc_rx) = std::sync::mpsc::channel::<String>();
    let has_ipc = host.ipc.is_some();
    let mut ipc_handler = host.ipc.map(|c| c.on_invoke);

    let scheme = host.scheme.clone();
    let request_handler = host.request_handler;
    let navigation_handler = host.navigation_handler;
    let init_script = host.init_script;

    let url = format!("{}://{}/{}", host.scheme, host.host, host.initial_url_path);

    let mut builder = WebViewBuilder::new()
        .with_url(url)
        .with_custom_protocol(scheme, move |_id, request| request_handler(&request))
        .with_navigation_handler(move |uri| navigation_handler(&uri));

    if let Some(script) = init_script.as_deref() {
        builder = builder.with_initialization_script(script);
    }
    if has_ipc {
        builder = builder.with_ipc_handler(move |request: Request<String>| {
            let _ = ipc_tx.send(request.body().clone());
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

    while running.get() {
        gtk::main_iteration_do(true);
        if let Some(handler) = ipc_handler.as_mut() {
            while let Ok(body) = ipc_rx.try_recv() {
                if let Some(script) = handler(body) {
                    if let Err(e) = webview.evaluate_script(&script) {
                        log::warn!("webview_host: reply eval failed: {e}");
                    }
                }
            }
        }
    }
    Ok(())
}
