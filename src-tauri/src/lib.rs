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
