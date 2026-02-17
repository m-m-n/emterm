//! ANSI escape sequence module.
//!
//! Provides APC and DCS parsers for image protocol handling.
//! The main ANSI parser state machine has been moved to the WASM crate
//! (`wasm/src/parser.rs`) for frontend-side processing.

pub mod apc;
pub mod dcs;

// Re-export public types used by image processing
pub use apc::{ApcAction, KittyAction, KittyCommand, KittyFormat};
pub use dcs::{DcsAction, SixelData};
