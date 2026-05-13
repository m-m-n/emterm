use tao::event_loop::EventLoop;

mod app;
mod callbacks;
mod image;
mod logging;
mod render;
mod tabs;
mod window_host;

// Phase 2+ modules — declared but empty for now so the tree compiles when
// later phases land. (Each file is a stub describing its responsibility.)
mod mux;
mod pty;
mod selection;
mod settings;
mod ui;
mod viewer;

fn main() {
    logging::init();
    log::info!("native-poc starting");

    let event_loop = EventLoop::new();
    let app = app::App::new();
    window_host::run(event_loop, app);
}
