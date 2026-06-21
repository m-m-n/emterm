//! Library facade for the `emterm` binary.
//!
//! All modules are declared here so that:
//! 1. The `cli` module tree (markdown / json / yaml / image subcommand
//!    handlers) is reachable from integration tests under `tests/`.
//! 2. The binary entry point in `main.rs` can pull modules in via
//!    `use emterm::*;` without duplicating every `mod ...;` declaration.
//!
//! The `gui` feature (default-on) toggles the windowed terminal stack
//! (winit + wgpu + egui + wry child WebViews). Disabling it
//! (`--no-default-features`) yields a CLI-only library exposing just the
//! subcommand dispatcher and the settings primitives it needs.

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
/// place (FR5 / NFR4). `with_name` exists on both the X11 and Wayland
/// extension traits, so each is invoked via fully-qualified syntax to
/// avoid an ambiguous method call; winit applies whichever matches the
/// active backend and ignores the other.
#[cfg(all(feature = "gui", target_os = "linux"))]
pub mod linux_wm {
    use winit::platform::wayland::WindowAttributesExtWayland;
    use winit::platform::x11::WindowAttributesExtX11;
    use winit::window::WindowAttributes;

    /// Set the X11 `WM_CLASS` and Wayland `app_id` to [`super::APP_WM_ID`].
    pub fn with_app_id(attrs: WindowAttributes) -> WindowAttributes {
        let id = super::APP_WM_ID;
        // X11: (general/class, instance). Wayland: app_id from `general`.
        let attrs = WindowAttributesExtX11::with_name(attrs, id, id);
        WindowAttributesExtWayland::with_name(attrs, id, id)
    }
}

// === CLI-shared modules (always built) ===

pub mod cli;
pub mod i18n;
pub mod localtime;
pub mod logging;
pub mod settings_core;

// === GUI-only modules (gated behind the `gui` feature) ===

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
pub mod scroll;
#[cfg(feature = "gui")]
pub mod search;
#[cfg(feature = "gui")]
pub mod sftp;
#[cfg(feature = "gui")]
pub mod tabs;
#[cfg(feature = "gui")]
pub mod window_host;

#[cfg(feature = "gui")]
pub mod ime;
#[cfg(feature = "gui")]
pub mod mux;
#[cfg(feature = "gui")]
pub mod pty;
#[cfg(feature = "gui")]
pub mod selection;
#[cfg(feature = "gui")]
pub mod self_exec;
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
pub mod wakeup;
#[cfg(feature = "gui")]
pub mod webview_host;

#[cfg(test)]
mod tests {
    // TS-4: the canonical dock-grouping identifier is `emterm`, matching
    // `emterm.desktop` / `StartupWMClass=emterm` (FR5 / NFR4).
    #[test]
    fn app_wm_id_is_emterm() {
        assert_eq!(super::APP_WM_ID, "emterm");
    }
}
