mod cell;
mod terminal_core;
mod unicode;

// Parser modules
mod parser;
mod parser_params;
mod parser_types;

// Handler modules (impl TerminalCore in separate files)
mod apc_handler;
mod c0_handler;
mod callbacks;
mod csi_cursor;
mod csi_device;
mod csi_dispatch;
mod csi_edit;
mod csi_modes;
mod csi_screen;
mod csi_scroll;
mod esc_handler;
mod osc_handler;
mod print_handler;
mod ring_buffer;
mod sgr;

use wasm_bindgen::prelude::*;

/// Trivial function to verify WASM pipeline works.
#[wasm_bindgen]
pub fn ping() -> u32 {
    42
}

// ── Single-codepoint APIs ───────────────────────────────

/// Get the display width of a codepoint (0, 1, or 2).
#[wasm_bindgen]
pub fn char_width(cp: u32) -> u8 {
    unicode::char_width(cp)
}

/// Pack all Unicode properties into a single byte for a codepoint.
#[wasm_bindgen]
pub fn classify_codepoint(cp: u32) -> u8 {
    unicode::classify_codepoint(cp)
}

// ── Individual property checks ──────────────────────────

#[wasm_bindgen]
pub fn is_emoji_presentation(cp: u32) -> bool {
    unicode::is_emoji_presentation(cp)
}

#[wasm_bindgen]
pub fn is_extended_pictographic(cp: u32) -> bool {
    unicode::is_extended_pictographic(cp)
}

#[wasm_bindgen]
pub fn is_regional_indicator(cp: u32) -> bool {
    unicode::is_regional_indicator(cp)
}

#[wasm_bindgen]
pub fn is_skin_tone_modifier(cp: u32) -> bool {
    unicode::is_skin_tone_modifier(cp)
}

#[wasm_bindgen]
pub fn is_variation_selector(cp: u32) -> bool {
    unicode::is_variation_selector(cp)
}

#[wasm_bindgen]
pub fn is_combining_char(cp: u32) -> bool {
    unicode::is_combining_char(cp)
}

#[wasm_bindgen]
pub fn is_ambiguous_width(cp: u32) -> bool {
    unicode::is_ambiguous_width(cp)
}

// ── Batch / string APIs ─────────────────────────────────

/// Classify all codepoints in a string, returning a packed byte per codepoint.
#[wasm_bindgen]
pub fn classify_codepoints(text: &str) -> Vec<u8> {
    unicode::classify_codepoints(text)
}

/// Calculate the display width of a string.
#[wasm_bindgen]
pub fn string_width(text: &str) -> u32 {
    unicode::string_width(text)
}
