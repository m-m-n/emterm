//! Terminal multiplexer module.
//!
//! Provides daemon process, IPC communication, and session management
//! for the eMterm native multiplexer.

mod bridge;
pub mod cli;
pub mod daemon;
pub mod ipc;
pub mod scrollback_buffer;
pub mod session;
pub mod snapshot;
pub mod tmux_conf;
mod tmux_import;
