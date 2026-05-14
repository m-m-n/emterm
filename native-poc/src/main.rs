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

fn main() {
    logging::init();
    log::info!("native-poc starting (winit 0.30 backend)");

    let event_loop = EventLoop::new().expect("native-poc: failed to create winit event loop");
    let app = app::App::new();
    window_host::run(event_loop, app);
}
