//! Terminal multiplexer module.
//!
//! Provides daemon process, IPC communication, and session management
//! for the eMterm native multiplexer.

pub mod bridge;
pub mod cli;
pub mod daemon;
pub mod ipc;
pub mod ring_buffer;
pub mod session;
pub mod snapshot;
