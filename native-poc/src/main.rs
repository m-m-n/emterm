use winit::event_loop::EventLoop;

mod app;
mod callbacks;
mod image;
mod logging;
mod render;
mod tabs;
mod window_host;

// Phase 2+ modules — declared but empty for now so the tree compiles when
// later phases land. (Each file is a stub describing its responsibility.)
mod ime;
mod mux;
mod pty;
mod selection;
mod settings;
mod ui;
mod viewer;
mod wakeup;

fn main() {
    logging::init();
    log::info!("native-poc starting (winit 0.30 backend)");

    // Hybrid PoC: wry's child WebView (the tab bar) hosts a WebKitGTK
    // widget that requires a live GTK main loop on Linux. We initialise
    // it before winit so the WebView builder can succeed during
    // `ApplicationHandler::resumed`. `events_pending` / `main_iteration_do`
    // get pumped from the winit `about_to_wait` callback.
    #[cfg(any(
        target_os = "linux",
        target_os = "dragonfly",
        target_os = "freebsd",
        target_os = "netbsd",
        target_os = "openbsd",
    ))]
    if let Err(e) = gtk::init() {
        log::warn!("native-poc: gtk::init failed: {e}. The WebView tab bar will be unavailable.");
    }

    let event_loop = EventLoop::new().expect("native-poc: failed to create winit event loop");
    // Install a cross-thread wakeup so PTY readers can pull the main
    // event loop out of `WaitUntil` the instant new bytes arrive,
    // instead of waiting on the 16 ms idle deadline that Wayland often
    // misses when the surface has nothing else to draw.
    let proxy = event_loop.create_proxy();
    wakeup::install(Box::new(move || {
        let _ = proxy.send_event(());
    }));
    let app = app::App::new();
    window_host::run(event_loop, app);
}
