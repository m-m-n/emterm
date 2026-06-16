// Suppress the console window on Windows for release builds. Without this
// attribute the binary links against the console subsystem, so launching it
// spawns a console window (Windows Terminal when set as the default) alongside
// the app window. Debug builds keep the console so log output stays visible.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use winit::event_loop::EventLoop;

// All modules live in the library target (`src/lib.rs`); the binary is a
// thin entry point that wires the event loop and dispatches to the
// library's subsystems. See `native-poc/Cargo.toml` for the `[lib]`
// target definition and `src/lib.rs` for the module roster.
use emterm_native_poc::{
    app, logging, mux, settings, settings_window, viewer, wakeup, window_host,
};

/// Build the winit event loop, preferring the X11 backend on Linux when a
/// Wayland session also exposes X11 (XWayland).
///
/// winit 0.30's Wayland backend does not emit `WindowEvent::DroppedFile` /
/// `HoveredFile` (only X11 / Windows / macOS do), so file drag-and-drop — the
/// SFTP upload entry point — is dead under native Wayland. When both
/// `WAYLAND_DISPLAY` and `DISPLAY` are set we force the X11 backend so drops
/// work via XWayland. Remove this once winit lands Wayland DnD
/// (rust-windowing/winit#1881 / PR #2429).
///
/// Override with `EMTERM_BACKEND=wayland` (keep native Wayland, no file drop)
/// or `EMTERM_BACKEND=x11` (force X11 whenever `DISPLAY` is set).
#[cfg(target_os = "linux")]
fn build_event_loop() -> EventLoop<()> {
    use winit::platform::x11::EventLoopBuilderExtX11;

    let non_empty = |k: &str| std::env::var_os(k).is_some_and(|v| !v.is_empty());
    let backend = std::env::var("EMTERM_BACKEND").unwrap_or_default();
    let has_wayland = non_empty("WAYLAND_DISPLAY") || non_empty("WAYLAND_SOCKET");
    let has_x11 = non_empty("DISPLAY");

    let force_x11 = match backend.as_str() {
        "wayland" => false,
        "x11" => has_x11,
        // auto: prefer X11 only when a Wayland session would otherwise be
        // selected (winit picks Wayland first) AND XWayland is available.
        _ => has_wayland && has_x11,
    };

    let mut builder = EventLoop::builder();
    if force_x11 {
        builder.with_x11();
        log::info!(
            "native-poc: forcing X11 backend (XWayland) so file drag-and-drop works \
             — winit Wayland DnD is unimplemented; set EMTERM_BACKEND=wayland to opt out"
        );
    }
    builder
        .build()
        .expect("native-poc: failed to create winit event loop")
}

#[cfg(not(target_os = "linux"))]
fn build_event_loop() -> EventLoop<()> {
    EventLoop::new().expect("native-poc: failed to create winit event loop")
}

fn main() {
    logging::init();

    let args: Vec<String> = std::env::args().collect();

    // CLI subcommand dispatch (markdown / json / yaml / image). Bare-word
    // subcommands are recognized first; `--`-prefixed child-process flags
    // (handled below) retain their existing hand-rolled path.
    if let Some(sub) = args.get(1).map(|s| s.as_str()) {
        if matches!(sub, "markdown" | "json" | "yaml" | "image") {
            let code = emterm_native_poc::cli::run(&args[1..]);
            std::process::exit(code);
        }
    }

    // `--viewer <payload-path>` dispatches to the separate child viewer
    // entry (Linux GTK/Wry window) before any terminal startup. The normal
    // terminal path is taken when the flag is absent.
    if let Some(pos) = args.iter().position(|a| a == "--viewer") {
        let payload_path = args.get(pos + 1).cloned();
        run_viewer(payload_path);
        return;
    }
    // `--image-viewer <payload-path>` runs the native (winit + wgpu + egui)
    // image viewer child window. Cross-platform, unlike the Wry Markdown
    // viewer above.
    if let Some(pos) = args.iter().position(|a| a == "--image-viewer") {
        let payload_path = args.get(pos + 1).cloned();
        run_image_viewer(payload_path);
        return;
    }
    // `--data-viewer <payload-path>` runs the native JSON/YAML data viewer
    // child window (same native stack as the image viewer).
    if let Some(pos) = args.iter().position(|a| a == "--data-viewer") {
        let payload_path = args.get(pos + 1).cloned();
        run_data_viewer(payload_path);
        return;
    }
    // `--settings` runs the child settings window (GTK/Wry, reused WebView
    // settings panel). No payload: the child reads settings.json itself.
    if args.iter().any(|a| a == "--settings") {
        run_settings_window();
        return;
    }

    log::info!("native-poc starting (winit 0.30 backend)");

    let event_loop = build_event_loop();
    // Install a cross-thread wakeup so PTY readers can pull the main
    // event loop out of `WaitUntil` the instant new bytes arrive,
    // instead of waiting on the 16 ms idle deadline that Wayland often
    // misses when the surface has nothing else to draw.
    let proxy = event_loop.create_proxy();
    wakeup::install(Box::new(move || {
        let _ = proxy.send_event(());
    }));
    // One-shot tmux.conf auto-import. Runs before the settings loader
    // reads the file so an imported `mux.prefix` / `mux.keybinds` etc.
    // are visible on this very launch. The function is idempotent
    // (latched on `mux.tmux_conf_imported`).
    mux::tmux_import::import_tmux_conf_if_needed();
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

/// Entry for the child `--viewer <payload-path>` process. Runs the Wry
/// Markdown viewer window (GTK/WebKitGTK on Linux, WebView2 on Windows)
/// and blocks until it closes. A missing payload path is a usage error.
fn run_viewer(payload_path: Option<String>) {
    let Some(path) = payload_path else {
        log::error!("--viewer requires a payload file path");
        std::process::exit(2);
    };

    match viewer::window::run(&path) {
        Ok(()) => log::info!("viewer: window closed"),
        Err(e) => {
            log::error!("{e}");
            std::process::exit(1);
        }
    }
}

/// Entry for the child `--settings` process. On Linux this runs the
/// GTK/Wry settings window; on Windows it runs the winit + wry/WebView2
/// equivalent. Other platforms have no implementation and exit non-zero.
fn run_settings_window() {
    #[cfg(any(target_os = "linux", target_os = "windows"))]
    {
        match settings_window::run() {
            Ok(()) => log::info!("settings window: closed"),
            Err(e) => {
                log::error!("{e}");
                std::process::exit(1);
            }
        }
    }
    #[cfg(not(any(target_os = "linux", target_os = "windows")))]
    {
        log::error!("settings window: --settings is not implemented on this platform");
        std::process::exit(1);
    }
}

/// Entry for the child `--image-viewer <payload-path>` process: a native
/// (winit + wgpu + egui) window showing one decoded image. Blocks until
/// the window closes. A missing payload path is a usage error.
fn run_image_viewer(payload_path: Option<String>) {
    let Some(path) = payload_path else {
        log::error!("--image-viewer requires a payload file path");
        std::process::exit(2);
    };
    match viewer::image_window::run(&path) {
        Ok(()) => log::info!("image viewer: window closed"),
        Err(e) => {
            log::error!("{e}");
            std::process::exit(1);
        }
    }
}

/// Entry for the child `--data-viewer <payload-path>` process: a native
/// JSON/YAML viewer window. Blocks until the window closes.
fn run_data_viewer(payload_path: Option<String>) {
    let Some(path) = payload_path else {
        log::error!("--data-viewer requires a payload file path");
        std::process::exit(2);
    };
    match viewer::data_window::run(&path) {
        Ok(()) => log::info!("data viewer: window closed"),
        Err(e) => {
            log::error!("{e}");
            std::process::exit(1);
        }
    }
}
