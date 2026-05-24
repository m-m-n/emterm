use winit::event_loop::EventLoop;

mod app;
mod callbacks;
mod html;
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
mod status_bar;
mod ui;
mod viewer;
mod wakeup;

fn main() {
    logging::init();
    log::info!("native-poc starting (winit 0.30 backend)");

    let event_loop = EventLoop::new().expect("native-poc: failed to create winit event loop");
    // Install a cross-thread wakeup so PTY readers can pull the main
    // event loop out of `WaitUntil` the instant new bytes arrive,
    // instead of waiting on the 16 ms idle deadline that Wayland often
    // misses when the surface has nothing else to draw.
    let proxy = event_loop.create_proxy();
    wakeup::install(Box::new(move || {
        let _ = proxy.send_event(());
    }));
    let settings = settings::Settings::load_or_default();
    let app = app::App::with_settings(settings);
    window_host::run(event_loop, app);
}
