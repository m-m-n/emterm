use winit::event_loop::EventLoop;

mod app;
mod bell;
mod callbacks;
mod fold;
mod html;
mod i18n;
mod image;
mod links;
mod localtime;
mod logging;
mod logical_line;
mod notifications;
mod prompts;
mod render;
mod search;
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

    // `--viewer <payload-path>` dispatches to the separate child viewer
    // entry (Linux GTK/Wry window) before any terminal startup. The normal
    // terminal path is taken when the flag is absent.
    let args: Vec<String> = std::env::args().collect();
    if let Some(pos) = args.iter().position(|a| a == "--viewer") {
        let payload_path = args.get(pos + 1).cloned();
        run_viewer(payload_path);
        return;
    }

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
    // Mirror src-tauri's setup: the `emterm.log` file handle only exists
    // in release builds; `log_recording_enabled` gates the writes.
    if !cfg!(debug_assertions) {
        logging::init_log_file();
    }
    logging::set_recording_enabled(settings.log_recording_enabled);
    let app = app::App::with_settings(settings);
    window_host::run(event_loop, app);
}

/// Entry for the child `--viewer <payload-path>` process. On Linux this
/// runs the GTK/Wry Markdown viewer window and blocks until it closes; on
/// other platforms the viewer window is not yet implemented, so we log and
/// exit non-zero. A missing payload path is a usage error.
fn run_viewer(payload_path: Option<String>) {
    let Some(path) = payload_path else {
        log::error!("--viewer requires a payload file path");
        std::process::exit(2);
    };

    #[cfg(target_os = "linux")]
    {
        match viewer::window::run(&path) {
            Ok(()) => log::info!("viewer: window closed"),
            Err(e) => {
                log::error!("{e}");
                std::process::exit(1);
            }
        }
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = path;
        log::error!("viewer: --viewer window is only implemented on Linux");
        std::process::exit(1);
    }
}
