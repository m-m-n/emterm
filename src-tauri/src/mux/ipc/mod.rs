pub mod codec;
pub mod connection;
pub mod handlers;
pub mod pty_spawn;
pub mod reattach;
pub mod statusbar;

// Re-export the wire protocol from the workspace crate so legacy
// `crate::mux::ipc::protocol::*` paths continue to resolve while the
// canonical definitions live in `crates/mux_ipc/`.
pub use mux_ipc::protocol;
