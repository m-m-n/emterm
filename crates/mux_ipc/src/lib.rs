//! mux_ipc: shared wire protocol data types for emterm mux daemon.
//!
//! This crate hosts the protocol-only portion of the mux IPC stack so that
//! both the legacy Tauri build (`src-tauri`) and `native-poc` can speak the
//! same wire format without dragging server-side tokio_util glue across the
//! crate boundary. Server framing (`codec`, `connection`) intentionally
//! remains in `src-tauri/src/mux/ipc/`.

pub mod protocol;
