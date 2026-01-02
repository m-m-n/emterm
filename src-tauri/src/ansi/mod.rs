//! ANSI escape sequence parser module.
//!
//! This module provides functionality for parsing ANSI escape sequences from
//! terminal output. It implements a state machine that handles CSI, ESC, and OSC
//! sequences, emitting structured `TerminalAction` values for each recognized
//! sequence.
//!
//! # Architecture
//!
//! The parser is designed to handle streaming input efficiently:
//! - Maintains state between calls to handle sequences split across buffers
//! - Emits actions via callback to avoid allocation overhead
//! - Supports all common terminal escape sequences
//!
//! # Example
//!
//! ```
//! use app_lib::ansi::{Parser, TerminalAction, CsiAction};
//!
//! let mut parser = Parser::new();
//! let mut actions = Vec::new();
//!
//! // Parse some ANSI-encoded text
//! parser.parse(b"\x1B[31mRed Text\x1B[0m", |action| {
//!     actions.push(action);
//! });
//!
//! // First action should be SGR for red foreground
//! assert!(matches!(&actions[0], TerminalAction::Csi(CsiAction::Sgr(_))));
//! ```
//!
//! # Module Structure
//!
//! - `parser`: State machine implementation (exports [`Parser`])
//! - `sequence`: Action type definitions (exports [`TerminalAction`], [`CsiAction`], etc.)
//! - `params`: CSI parameter parsing utilities (exports [`ParamParser`])
//! - [`sgr`]: SGR (Select Graphic Rendition) attribute parsing

mod params;
mod parser;
mod sequence;
pub mod sgr;

// Re-export public types
pub use params::ParamParser;
pub use parser::Parser;
pub use sequence::{CharSet, CsiAction, EraseMode, EscAction, OscAction, TerminalAction};
pub use sgr::{Color, SgrAttr, parse_sgr};
