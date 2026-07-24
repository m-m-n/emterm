//! Library facade for the `emterm` binary.
//!
//! All modules are declared here so that:
//! 1. The `cli` module tree (markdown / json / yaml / image subcommand
//!    handlers) is reachable from integration tests under `tests/`.
//! 2. The binary entry point in `main.rs` can pull modules in via
//!    `use emterm::*;` without duplicating every `mod ...;` declaration.
//!
//! The `gui` feature (default-on) toggles the windowed terminal stack
//! (winit + wgpu + egui + wry child WebViews) and its companion modules
//! (font rasterizer, settings UI, image viewer, etc.). Disabling it
//! (`--no-default-features`) yields a CLI library that still exposes
//! the mux daemon / bridge / PTY pipeline plus the `markdown` / `json`
//! / `yaml` / `image` subcommand dispatchers — only the windowed
//! terminal stack is dropped.

/// Canonical application identifier reported by every window so the Linux
/// desktop groups them under a single dock icon (FR5 / NFR4).
///
/// This is the **single source of truth** for the identifier. winit
/// windows (main terminal, image viewer, JSON/YAML data viewer) set it as
/// the X11 `WM_CLASS` / Wayland `app_id`; the GTK child windows (settings,
/// Markdown viewer) report it via the program identity GTK derives those
/// from. The value matches the installed desktop entry `emterm.desktop`
/// and its `StartupWMClass=emterm`, so GNOME/Ubuntu associates every
/// window with that one desktop entry and dock icon.
pub const APP_WM_ID: &str = "emterm";

/// Linux-only helper that stamps [`APP_WM_ID`] onto a winit
/// [`WindowAttributes`](winit::window::WindowAttributes) as both the X11
/// `WM_CLASS` and the Wayland `app_id`.
///
/// Every winit window (main terminal, image viewer, JSON/YAML data
/// viewer) routes through here so the identifier is set in exactly one
/// place (FR5 / NFR4). winit 0.31 attaches platform-specific attributes
/// through `WindowAttributes::with_platform_attributes`, which requires
/// knowing which backend is active, so the active event loop decides
/// between the X11 and Wayland attribute builders at call time.
#[cfg(all(feature = "gui", target_os = "linux"))]
pub mod linux_wm {
    use winit::event_loop::ActiveEventLoop;
    use winit::platform::wayland::{ActiveEventLoopExtWayland, WindowAttributesWayland};
    use winit::platform::x11::{ActiveEventLoopExtX11, WindowAttributesX11};
    use winit::window::WindowAttributes;

    /// Set the X11 `WM_CLASS` and Wayland `app_id` to [`super::APP_WM_ID`].
    pub fn with_app_id(
        event_loop: &dyn ActiveEventLoop,
        attrs: WindowAttributes,
    ) -> WindowAttributes {
        let id = super::APP_WM_ID;
        // X11: (general/class, instance). Wayland: app_id from `general`.
        if event_loop.is_x11() {
            attrs.with_platform_attributes(Box::new(
                WindowAttributesX11::default().with_name(id, id),
            ))
        } else if event_loop.is_wayland() {
            attrs.with_platform_attributes(Box::new(
                WindowAttributesWayland::default().with_name(id, id),
            ))
        } else {
            attrs
        }
    }
}

/// The Linux event-loop backend decision (FR2). Wayland is the default;
/// `EMTERM_BACKEND` can force either backend explicitly.
///
/// A library module (not inline in `main.rs`) so [`decide_backend`]'s unit
/// tests run under `cargo test --lib` — this crate's binary target has no
/// test harness of its own.
#[cfg(all(feature = "gui", target_os = "linux"))]
pub mod backend_select {
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum Backend {
        /// No backend forced — winit auto-selects (Wayland preferred).
        Auto,
        ForceWayland,
        ForceX11,
    }

    /// Pure decision function for the Linux backend selection (FR2).
    ///
    /// `env_value` is the raw `EMTERM_BACKEND` value (may be empty/unknown).
    /// `has_wayland` / `has_x11` are the presence flags of a Wayland session
    /// (`WAYLAND_DISPLAY` / `WAYLAND_SOCKET` non-empty) and an X11 display
    /// (`DISPLAY` non-empty), respectively — kept as inputs to mirror the
    /// IMPLEMENTATION.md contract even though only `has_x11` currently
    /// changes the outcome.
    ///
    /// - `"wayland"` -> [`Backend::ForceWayland`]
    /// - `"x11"` with `has_x11` -> [`Backend::ForceX11`]
    /// - anything else (`"x11"` without X11 present, empty, unknown) ->
    ///   [`Backend::Auto`]
    pub fn decide_backend(env_value: &str, _has_wayland: bool, has_x11: bool) -> Backend {
        match env_value {
            "wayland" => Backend::ForceWayland,
            "x11" if has_x11 => Backend::ForceX11,
            _ => Backend::Auto,
        }
    }

    #[cfg(test)]
    mod tests {
        use super::{Backend, decide_backend};

        // AC-2: EMTERM_BACKEND=wayland -> ForceWayland, regardless of
        // presence flags.
        #[test]
        fn wayland_env_forces_wayland() {
            assert_eq!(
                decide_backend("wayland", false, false),
                Backend::ForceWayland
            );
            assert_eq!(decide_backend("wayland", true, true), Backend::ForceWayland);
        }

        // AC-2: EMTERM_BACKEND=x11 with X11 present -> ForceX11.
        #[test]
        fn x11_env_with_x11_present_forces_x11() {
            assert_eq!(decide_backend("x11", false, true), Backend::ForceX11);
            assert_eq!(decide_backend("x11", true, true), Backend::ForceX11);
        }

        // AC-2: EMTERM_BACKEND=x11 without X11 present -> Auto.
        #[test]
        fn x11_env_without_x11_present_is_auto() {
            assert_eq!(decide_backend("x11", false, false), Backend::Auto);
            assert_eq!(decide_backend("x11", true, false), Backend::Auto);
        }

        // AC-2: empty EMTERM_BACKEND -> Auto.
        #[test]
        fn empty_env_is_auto() {
            assert_eq!(decide_backend("", false, false), Backend::Auto);
            assert_eq!(decide_backend("", true, true), Backend::Auto);
        }

        // AC-2: unknown EMTERM_BACKEND values -> Auto.
        #[test]
        fn unknown_env_is_auto() {
            assert_eq!(decide_backend("bogus", false, false), Backend::Auto);
            assert_eq!(decide_backend("Wayland", false, true), Backend::Auto);
            assert_eq!(decide_backend("X11", false, true), Backend::Auto);
        }

        // AC-3: the Auto decision never carries a force-X11 (or
        // force-Wayland) intent — `build_event_loop()` in `main.rs` only
        // calls `with_x11()` / `with_wayland()` in the ForceX11 /
        // ForceWayland match arms, never for Auto.
        #[test]
        fn auto_decision_is_distinct_from_forced_variants() {
            let auto = decide_backend("", false, true);
            assert_ne!(auto, Backend::ForceX11);
            assert_ne!(auto, Backend::ForceWayland);
        }
    }
}

// === CLI-shared modules (always built) ===

pub mod agent_status;
pub mod cli;
pub mod i18n;
pub mod localtime;
pub mod logging;
pub mod settings_core;
pub mod viewer_kinds;

// === Mux subsystem modules (always built) ===
//
// The mux daemon / bridge / PTY pipeline have no GUI dependency, so they
// are part of every build — the CLI deb runs them on headless SSH hosts.
// `mux::tmux_import` (gated inside `mux/mod.rs`) is the only mux submodule
// that requires the `gui` feature, because it writes to the GUI-only
// `settings_store`.

pub mod mux;
pub mod pty;
pub mod scroll;
pub mod self_exec;
pub mod wakeup;

// Windows-only process resolution helpers (PE / shebang routing,
// CREATE_NO_WINDOW). Compiled on `test` too so the unit tests covering
// the shebang parser run on Linux CI.
#[cfg(any(windows, test))]
pub mod windows_exec;

// === GUI-only modules (gated behind the `gui` feature) ===

#[cfg(feature = "gui")]
pub mod agent_status_model;
#[cfg(feature = "gui")]
pub mod app;
#[cfg(feature = "gui")]
pub mod bell;
#[cfg(feature = "gui")]
pub mod callbacks;
#[cfg(feature = "gui")]
pub mod fold;
#[cfg(feature = "gui")]
pub mod html;
#[cfg(feature = "gui")]
pub mod image;
#[cfg(feature = "gui")]
pub mod links;
#[cfg(feature = "gui")]
pub mod logical_line;
#[cfg(feature = "gui")]
pub mod notifications;
#[cfg(feature = "gui")]
pub mod profiles;
#[cfg(feature = "gui")]
pub mod prompts;
#[cfg(feature = "gui")]
pub mod render;
#[cfg(feature = "gui")]
pub mod search;
#[cfg(feature = "gui")]
pub mod sftp;
#[cfg(feature = "gui")]
pub mod tabs;
// Discovery of live tmux sockets for the new-tab chooser's tmux-attach
// rows (SPEC A5). Unix-only: tmux's socket-directory layout and Unix-domain
// socket probing have no Windows equivalent; `app.rs`'s chooser plumbing
// stays platform-agnostic by dispatching to this module only on `cfg(unix)`
// and falling back to an empty socket list elsewhere.
#[cfg(all(feature = "gui", unix))]
pub mod tmux_sockets;
#[cfg(feature = "gui")]
pub mod window_host;

#[cfg(feature = "gui")]
pub mod ime;
#[cfg(feature = "gui")]
pub mod selection;
#[cfg(feature = "gui")]
pub mod settings;
#[cfg(feature = "gui")]
pub mod settings_launcher;
#[cfg(feature = "gui")]
pub mod settings_store;
#[cfg(feature = "gui")]
pub mod settings_window;
#[cfg(feature = "gui")]
pub mod status_bar;
#[cfg(feature = "gui")]
pub mod ui;
#[cfg(feature = "gui")]
pub mod viewer;
#[cfg(feature = "gui")]
pub mod webview_host;
#[cfg(feature = "gui")]
pub mod window_icon;

#[cfg(test)]
mod tests {
    // TS-4: the canonical dock-grouping identifier is `emterm`, matching
    // `emterm.desktop` / `StartupWMClass=emterm` (FR5 / NFR4).
    #[test]
    fn app_wm_id_is_emterm() {
        assert_eq!(super::APP_WM_ID, "emterm");
    }
}
