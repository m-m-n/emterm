//! Encoding utilities for CLI subcommand output.
//!
//! Ports `src-tauri/src/encoding/{base64,osc}.rs` verbatim. The byte
//! format of every OSC frame is preserved to maintain receiver-side
//! compatibility with the WebView build.

pub mod base64;
pub mod osc;
