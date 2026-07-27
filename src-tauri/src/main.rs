// Suppress the console window on Windows for release GUI builds. Without
// this attribute the binary links against the console subsystem, so
// launching it spawns a console window alongside the app window. Debug
// builds — and the CLI-only build (`--no-default-features`) — keep the
// console so log output stays visible.
#![cfg_attr(
    all(not(debug_assertions), feature = "gui"),
    windows_subsystem = "windows"
)]

use emterm::logging;

#[cfg(feature = "gui")]
use emterm::{app, mux, self_exec, settings, settings_window, viewer, wakeup, window_host};
#[cfg(feature = "gui")]
use winit::event_loop::EventLoop;

/// Build the winit event loop. Wayland is the Linux default; `EMTERM_BACKEND`
/// overrides to `wayland` (explicit, no-op) or `x11` (force X11 whenever
/// `DISPLAY` is set) per [`emterm::backend_select::decide_backend`]. The
/// decision logic itself lives in the library crate (`emterm::backend_select`)
/// rather than here so its unit tests run under `cargo test --lib` — this
/// crate's binary target has no test harness of its own.
#[cfg(all(feature = "gui", target_os = "linux"))]
fn build_event_loop() -> EventLoop {
    use emterm::backend_select::{Backend, decide_backend};
    use winit::platform::wayland::EventLoopBuilderExtWayland;
    use winit::platform::x11::EventLoopBuilderExtX11;

    let non_empty = |k: &str| std::env::var_os(k).is_some_and(|v| !v.is_empty());
    let backend_env = std::env::var("EMTERM_BACKEND").unwrap_or_default();
    let has_wayland = non_empty("WAYLAND_DISPLAY") || non_empty("WAYLAND_SOCKET");
    let has_x11 = non_empty("DISPLAY");
    let decision = decide_backend(&backend_env, has_wayland, has_x11);

    let mut builder = EventLoop::builder();
    match decision {
        Backend::ForceWayland => {
            builder.with_wayland();
            log::info!("emterm: backend decision = ForceWayland (EMTERM_BACKEND=wayland)");
        }
        Backend::ForceX11 => {
            builder.with_x11();
            log::info!("emterm: backend decision = ForceX11 (EMTERM_BACKEND=x11, DISPLAY set)");
        }
        Backend::Auto => {
            log::info!("emterm: backend decision = Auto (winit auto-select, Wayland preferred)");
        }
    }
    builder
        .build()
        .expect("emterm: failed to create winit event loop")
}

#[cfg(all(feature = "gui", not(target_os = "linux")))]
fn build_event_loop() -> EventLoop {
    EventLoop::new().expect("emterm: failed to create winit event loop")
}

fn main() {
    let args: Vec<String> = std::env::args().collect();

    // `--version` — print the compile-time crate version and exit, before
    // `logging::init()`, any settings read, or any `#[cfg(feature = "gui")]`
    // code (D1 in IMPLEMENTATION.md). Identical in the GUI and CLI-only
    // builds since it sits outside all feature gates. Only the first
    // argument position is inspected, so trailing arguments are ignored
    // (`emterm --version anything` still succeeds).
    if args.get(1).is_some_and(|a| a == "--version") {
        println!("{}", env!("CARGO_PKG_VERSION"));
        std::process::exit(0);
    }

    // CLI subcommand dispatch (markdown / json / yaml / image / mux).
    // Bare-word subcommands are recognized BEFORE `logging::init()` because
    // each subcommand owns its own logger (mux bridge / daemon write to
    // dedicated log files via `env_logger::Builder::init`, which panics if
    // a global logger was already installed). Subcommands that need a
    // logger install one themselves; the rest run unlogged.
    if let Some(sub) = args.get(1).map(|s| s.as_str()) {
        if matches!(
            sub,
            "markdown" | "json" | "yaml" | "image" | "html" | "agent-status"
        ) {
            let code = emterm::cli::run(&args[1..]);
            std::process::exit(code);
        }
        // `emterm mux …` — terminal multiplexer CLI bridge / daemon entry.
        // The mux subsystem ships in every build (GUI and CLI alike) so the
        // CLI deb on a headless SSH host can still run `emterm mux --daemon`.
        if sub == "mux" {
            let code = emterm::mux::cli::run(&args[2..]);
            std::process::exit(code);
        }
    }

    // No subcommand matched — proceed to the GUI / image-viewer / settings
    // path. The GUI owns the global logger.
    logging::init();

    #[cfg(feature = "gui")]
    {
        run_gui(args);
    }

    #[cfg(not(feature = "gui"))]
    {
        eprintln!(
            "emterm: this build provides only CLI subcommands.\n\
             Usage: emterm <markdown|json|yaml|html|image> <file> [options]\n\
             \x20      emterm agent-status <idle|working|blocked|done|clear> [--name <n>]\n\
             Run `emterm <subcommand> --help` for details."
        );
        std::process::exit(2);
    }
}

#[cfg(feature = "gui")]
fn run_gui(args: Vec<String>) {
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
    // `--html-viewer <payload-path>` runs the Wry HTML viewer child window
    // (Decision D1 in IMPLEMENTATION.md — its own flag/window module,
    // distinct from the Markdown `--viewer`, since the document is served
    // verbatim rather than injected into the bundled renderer).
    if let Some(pos) = args.iter().position(|a| a == "--html-viewer") {
        let payload_path = args.get(pos + 1).cloned();
        run_html_viewer(payload_path);
        return;
    }
    // `--settings` runs the child settings window (GTK/Wry, reused WebView
    // settings panel). No payload: the child reads settings.json itself.
    if args.iter().any(|a| a == "--settings") {
        run_settings_window();
        return;
    }

    log::info!("emterm starting (winit 0.31 backend)");

    let event_loop = build_event_loop();
    // Install a cross-thread wakeup so PTY readers can pull the main
    // event loop out of `WaitUntil` the instant new bytes arrive,
    // instead of waiting on the 16 ms idle deadline that Wayland often
    // misses when the surface has nothing else to draw.
    let proxy = event_loop.create_proxy();
    wakeup::install(Box::new(move || {
        proxy.wake_up();
    }));
    // Capture the self-binary baseline (path, device, inode) once, before any
    // self-spawn, so a later on-disk replacement is detectable. Must run after
    // `wakeup::install` so `note_spawn_failure` can wake the loop.
    self_exec::init();
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
#[cfg(feature = "gui")]
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
#[cfg(feature = "gui")]
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
#[cfg(feature = "gui")]
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
#[cfg(feature = "gui")]
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

/// Entry for the child `--html-viewer <payload-path>` process. Runs the Wry
/// HTML viewer window (GTK/WebKitGTK on Linux, WebView2 on Windows) and
/// blocks until it closes. A missing payload path is a usage error; a
/// missing/unreadable payload file logs an error and exits non-zero without
/// panicking (AC-1).
#[cfg(feature = "gui")]
fn run_html_viewer(payload_path: Option<String>) {
    let Some(path) = payload_path else {
        log::error!("--html-viewer requires a payload file path");
        std::process::exit(2);
    };
    match viewer::html_window::run(&path) {
        Ok(()) => log::info!("html viewer: window closed"),
        Err(e) => {
            log::error!("{e}");
            std::process::exit(1);
        }
    }
}
