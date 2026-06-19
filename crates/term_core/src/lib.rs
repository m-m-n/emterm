//! `term_core` — ANSI parser + terminal grid + Unicode processing as a
//! pure-Rust library.
//!
//! Originally lived under `wasm/src/` as a wasm-bindgen cdylib. Phase 2 of
//! `doc/tasks/term-core-rust-crate/` extracted the core out of the wasm
//! crate; the thin wrapper at `wasm/src/lib.rs` re-exposes this crate to
//! JavaScript via wasm-bindgen.
//!
//! The module layout is preserved from the previous `wasm/src/` tree
//! (NFR3); only the wasm-bindgen surface has been removed.

pub mod cell;
pub mod char_table;
pub mod color_spec;
pub mod slim_cell;
pub mod snapshot;
pub mod style_table;
pub mod terminal_cells;
pub mod terminal_core;
pub mod terminal_cursor;
pub mod terminal_dispatch;
pub mod terminal_modes;
pub mod terminal_rows;
pub mod unicode;
pub mod unicode_emoji;
pub mod unicode_width;

// Parser modules
pub mod parser;
pub mod parser_params;
pub mod parser_types;

// Mux inband transport extractor (independent parser state)
pub mod mux_apc_extractor;

// Bench harness (test-only)
#[cfg(test)]
mod bench;

// Handler modules (impl TerminalCore in separate files)
pub mod apc_handler;
pub mod c0_handler;
pub mod callbacks;
pub mod csi_cursor;
pub mod csi_device;
pub mod csi_dispatch;
pub mod csi_edit;
pub mod csi_modes;
pub mod csi_screen;
pub mod csi_scroll;
pub mod esc_handler;
pub mod osc_handler;
pub mod print_handler;
pub mod reflow;
pub mod ring_buffer;
pub mod sgr;

pub use callbacks::TerminalCallbacks;
pub use mux_apc_extractor::MuxApcExtractor;
pub use terminal_core::TerminalCore;

// ── Single-codepoint APIs ───────────────────────────────

/// Get the display width of a codepoint (0, 1, or 2).
pub fn char_width(cp: u32) -> u8 {
    unicode::char_width(cp)
}

/// Pack all Unicode properties into a single byte for a codepoint.
pub fn classify_codepoint(cp: u32) -> u8 {
    unicode::classify_codepoint(cp)
}

// ── Individual property checks ──────────────────────────

pub fn is_emoji_presentation(cp: u32) -> bool {
    unicode::is_emoji_presentation(cp)
}

pub fn is_extended_pictographic(cp: u32) -> bool {
    unicode::is_extended_pictographic(cp)
}

pub fn is_regional_indicator(cp: u32) -> bool {
    unicode::is_regional_indicator(cp)
}

pub fn is_skin_tone_modifier(cp: u32) -> bool {
    unicode::is_skin_tone_modifier(cp)
}

pub fn is_variation_selector(cp: u32) -> bool {
    unicode::is_variation_selector(cp)
}

pub fn is_combining_char(cp: u32) -> bool {
    unicode::is_combining_char(cp)
}

pub fn is_ambiguous_width(cp: u32) -> bool {
    unicode::is_ambiguous_width(cp)
}

// ── Batch / string APIs ─────────────────────────────────

/// Classify all codepoints in a string, returning a packed byte per codepoint.
pub fn classify_codepoints(text: &str) -> Vec<u8> {
    unicode::classify_codepoints(text)
}

/// Calculate the display width of a string.
pub fn string_width(text: &str) -> u32 {
    unicode::string_width(text)
}

/// Trivial health-check used by the thin wrapper for the previous `ping()`
/// JS export.
pub fn ping() -> u32 {
    42
}
