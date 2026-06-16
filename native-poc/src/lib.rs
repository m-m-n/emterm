//! Library facade for `emterm-native-poc`.
//!
//! All native-poc modules are declared here so that:
//! 1. The `cli` module tree (markdown / json / yaml / image subcommand
//!    handlers) is reachable from integration tests under `native-poc/tests/`.
//! 2. The binary entry point in `main.rs` can pull modules in via
//!    `use emterm_native_poc::*;` without duplicating every `mod ...;`
//!    declaration.

pub mod app;
pub mod bell;
pub mod callbacks;
pub mod cli;
pub mod fold;
pub mod html;
pub mod i18n;
pub mod image;
pub mod links;
pub mod localtime;
pub mod logging;
pub mod logical_line;
pub mod notifications;
pub mod profiles;
pub mod prompts;
pub mod render;
pub mod search;
pub mod sftp;
pub mod tabs;
pub mod window_host;

pub mod ime;
pub mod mux;
pub mod pty;
pub mod selection;
pub mod settings;
pub mod settings_launcher;
pub mod settings_store;
pub mod settings_window;
pub mod status_bar;
pub mod ui;
pub mod viewer;
pub mod wakeup;
pub mod webview_host;
