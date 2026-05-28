//! `emterm-wasm` — thin wasm-bindgen wrapper around `term_core`.
//!
//! Phase 2 of `doc/tasks/term-core-rust-crate/` moved the ANSI parser /
//! terminal grid / Unicode processing into the `term_core` crate. This
//! file re-exposes that crate to the TypeScript side at parity with the
//! previous direct wasm-bindgen build (the same JS function names,
//! parameter shapes, and return shapes).
//!
//! The wrapper:
//! - Defines `TerminalCore` as a `#[wasm_bindgen]` struct that owns a
//!   `term_core::TerminalCore` plus an `Rc<RefCell<JsCallbacks>>` shared
//!   with the trait sink installed on the core.
//! - Defines `JsCallbacks`, the storage for `js_sys::Function` handles.
//! - Defines `JsCallbackBridge`, a `term_core::TerminalCallbacks` impl that
//!   forwards trait method calls to the JS functions inside `JsCallbacks`.
//! - Re-exports the free Unicode helper functions.

use std::cell::RefCell;
use std::rc::Rc;

use js_sys::Function;
use wasm_bindgen::prelude::*;

use term_core::TerminalCallbacks;

// ── JS callback storage ─────────────────────────────────

/// Storage for the JS `Function` handles. The trait sink installed on
/// `term_core::TerminalCore` holds a clone of the `Rc<RefCell<_>>` so
/// setters mutate exactly one underlying instance.
#[derive(Default)]
struct JsCallbacks {
    osc: Option<Function>,
    apc: Option<Function>,
    dcs: Option<Function>,
    bell: Option<Function>,
    device_response: Option<Function>,
}

/// `TerminalCallbacks` impl that borrows the shared `JsCallbacks` for each
/// fire site and calls the corresponding `js_sys::Function`.
struct JsCallbackBridge {
    inner: Rc<RefCell<JsCallbacks>>,
}

impl TerminalCallbacks for JsCallbackBridge {
    fn on_osc(&self, action_type: u8, data: &str) {
        if let Some(cb) = self.inner.borrow().osc.clone() {
            let _ = cb.call2(
                &JsValue::NULL,
                &JsValue::from(action_type),
                &JsValue::from(data),
            );
        }
    }
    fn on_apc(&self, data: &[u8]) {
        if let Some(cb) = self.inner.borrow().apc.clone() {
            let array = js_sys::Uint8Array::from(data);
            let _ = cb.call1(&JsValue::NULL, &array);
        }
    }
    fn on_dcs(&self, data: &[u8]) {
        if let Some(cb) = self.inner.borrow().dcs.clone() {
            let array = js_sys::Uint8Array::from(data);
            let _ = cb.call1(&JsValue::NULL, &array);
        }
    }
    fn on_bell(&self) {
        if let Some(cb) = self.inner.borrow().bell.clone() {
            let _ = cb.call0(&JsValue::NULL);
        }
    }
    fn on_device_response(&self, data: &[u8]) {
        if let Some(cb) = self.inner.borrow().device_response.clone() {
            let array = js_sys::Uint8Array::from(data);
            let _ = cb.call1(&JsValue::NULL, &array);
        }
    }
}

// ── TerminalCore: thin wrapper ──────────────────────────

#[wasm_bindgen]
pub struct TerminalCore {
    inner: term_core::TerminalCore,
    callbacks: Rc<RefCell<JsCallbacks>>,
}

impl TerminalCore {
    fn install_callbacks(inner: &mut term_core::TerminalCore) -> Rc<RefCell<JsCallbacks>> {
        let storage: Rc<RefCell<JsCallbacks>> = Rc::new(RefCell::new(JsCallbacks::default()));
        inner.callbacks = Some(Box::new(JsCallbackBridge {
            inner: Rc::clone(&storage),
        }));
        storage
    }
}

#[wasm_bindgen]
impl TerminalCore {
    #[wasm_bindgen(constructor)]
    pub fn new(cols: u16, rows: u16, scrollback_lines: u32) -> Self {
        let mut inner = term_core::TerminalCore::new(cols, rows, scrollback_lines);
        let callbacks = Self::install_callbacks(&mut inner);
        Self { inner, callbacks }
    }

    // ── Callback setters ────────────────────────────────

    pub fn set_osc_callback(&mut self, callback: JsValue) {
        self.callbacks.borrow_mut().osc = callback.dyn_into::<Function>().ok();
    }
    pub fn set_apc_callback(&mut self, callback: JsValue) {
        self.callbacks.borrow_mut().apc = callback.dyn_into::<Function>().ok();
    }
    pub fn set_dcs_callback(&mut self, callback: JsValue) {
        self.callbacks.borrow_mut().dcs = callback.dyn_into::<Function>().ok();
    }
    pub fn set_bell_callback(&mut self, callback: JsValue) {
        self.callbacks.borrow_mut().bell = callback.dyn_into::<Function>().ok();
    }
    pub fn set_device_response_callback(&mut self, callback: JsValue) {
        self.callbacks.borrow_mut().device_response = callback.dyn_into::<Function>().ok();
    }
    pub fn clear_callbacks(&mut self) {
        *self.callbacks.borrow_mut() = JsCallbacks::default();
    }

    // ── Dimensions ──────────────────────────────────────

    pub fn cols(&self) -> u16 {
        self.inner.cols()
    }
    pub fn rows(&self) -> u16 {
        self.inner.rows()
    }
    pub fn resize(&mut self, new_cols: u16, new_rows: u16) {
        self.inner.resize(new_cols, new_rows);
    }
    pub fn resize_no_reflow(&mut self, new_cols: u16, new_rows: u16) {
        self.inner.resize_no_reflow(new_cols, new_rows);
    }
    pub fn resize_reflow(&mut self, new_cols: u16, new_rows: u16, scrollback_lines: u32) -> u32 {
        self.inner
            .resize_reflow(new_cols, new_rows, scrollback_lines)
    }

    pub fn set_cell_size_px(&mut self, width: u16, height: u16) {
        self.inner.set_cell_size_px(width, height);
    }
    pub fn get_cell_width_px(&self) -> u16 {
        self.inner.get_cell_width_px()
    }
    pub fn get_cell_height_px(&self) -> u16 {
        self.inner.get_cell_height_px()
    }

    // ── Scroll event ────────────────────────────────────

    pub fn get_scroll_event_direction(&self) -> u8 {
        self.inner.get_scroll_event_direction()
    }
    pub fn get_scroll_event_count(&self) -> u16 {
        self.inner.get_scroll_event_count()
    }
    pub fn clear_scroll_event(&mut self) {
        self.inner.clear_scroll_event();
    }

    // ── Reset / actions ─────────────────────────────────

    pub fn reset(&mut self) {
        self.inner.reset();
    }
    pub fn take_mode_actions(&mut self) -> Vec<u8> {
        self.inner.take_mode_actions()
    }
    pub fn set_cursor_show_interrupt(&mut self, enable: bool) {
        self.inner.set_cursor_show_interrupt(enable);
    }

    // ── Charsets ────────────────────────────────────────

    pub fn get_g0_charset(&self) -> u8 {
        self.inner.get_g0_charset()
    }
    pub fn set_g0_charset(&mut self, val: u8) {
        self.inner.set_g0_charset(val);
    }
    pub fn get_g1_charset(&self) -> u8 {
        self.inner.get_g1_charset()
    }
    pub fn set_g1_charset(&mut self, val: u8) {
        self.inner.set_g1_charset(val);
    }
    pub fn get_active_charset(&self) -> u8 {
        self.inner.get_active_charset()
    }
    pub fn set_active_charset(&mut self, val: u8) {
        self.inner.set_active_charset(val);
    }

    // ── Scroll region ───────────────────────────────────

    pub fn get_scroll_region_top(&self) -> u16 {
        self.inner.get_scroll_region_top()
    }
    pub fn get_scroll_region_bottom(&self) -> u16 {
        self.inner.get_scroll_region_bottom()
    }
    pub fn set_scroll_region(&mut self, top: u16, bottom: u16) {
        self.inner.set_scroll_region(top, bottom);
    }

    // ── Wrap pending / grapheme buffer ──────────────────

    pub fn get_wrap_pending(&self) -> bool {
        self.inner.get_wrap_pending()
    }
    pub fn set_wrap_pending(&mut self, val: bool) {
        self.inner.set_wrap_pending(val);
    }
    pub fn get_grapheme_buffer_len(&self) -> u32 {
        self.inner.get_grapheme_buffer_len()
    }
    pub fn clear_grapheme_buffer(&mut self) {
        self.inner.clear_grapheme_buffer();
    }
    pub fn flush_grapheme_buffer(&mut self) -> u8 {
        self.inner.flush_grapheme_buffer()
    }

    // ── Cursor ──────────────────────────────────────────

    pub fn get_cursor_col(&self) -> u16 {
        self.inner.get_cursor_col()
    }
    pub fn get_cursor_row(&self) -> u16 {
        self.inner.get_cursor_row()
    }
    pub fn set_cursor(&mut self, col: u16, row: u16) {
        self.inner.set_cursor(col, row);
    }
    pub fn set_cursor_col(&mut self, col: u16) {
        self.inner.set_cursor_col(col);
    }
    pub fn set_cursor_row(&mut self, row: u16) {
        self.inner.set_cursor_row(row);
    }
    pub fn get_cursor_visible(&self) -> bool {
        self.inner.get_cursor_visible()
    }
    pub fn set_cursor_visible(&mut self, visible: bool) {
        self.inner.set_cursor_visible(visible);
    }
    pub fn get_cursor_style(&self) -> u8 {
        self.inner.get_cursor_style()
    }
    pub fn set_cursor_style(&mut self, style: u8) {
        self.inner.set_cursor_style(style);
    }
    pub fn get_cursor_blink(&self) -> bool {
        self.inner.get_cursor_blink()
    }
    pub fn set_cursor_blink(&mut self, blink: bool) {
        self.inner.set_cursor_blink(blink);
    }
    pub fn get_cursor_fg(&self) -> u32 {
        self.inner.get_cursor_fg()
    }
    pub fn set_cursor_fg(&mut self, tag: u8, r: u8, g: u8, b: u8) {
        self.inner.set_cursor_fg(tag, r, g, b);
    }
    pub fn get_cursor_bg(&self) -> u32 {
        self.inner.get_cursor_bg()
    }
    pub fn set_cursor_bg(&mut self, tag: u8, r: u8, g: u8, b: u8) {
        self.inner.set_cursor_bg(tag, r, g, b);
    }
    pub fn get_cursor_flags(&self) -> u16 {
        self.inner.get_cursor_flags()
    }
    pub fn set_cursor_flags(&mut self, flags: u16) {
        self.inner.set_cursor_flags(flags);
    }
    pub fn reset_cursor_attrs(&mut self) {
        self.inner.reset_cursor_attrs();
    }
    pub fn save_cursor(&mut self) {
        self.inner.save_cursor();
    }
    pub fn restore_cursor(&mut self) {
        self.inner.restore_cursor();
    }

    // ── Cells ───────────────────────────────────────────

    #[allow(clippy::too_many_arguments)]
    pub fn set_cell(
        &mut self,
        col: u16,
        row: u16,
        char_str: &str,
        width: u8,
        fg_tag: u8,
        fg_r: u8,
        fg_g: u8,
        fg_b: u8,
        bg_tag: u8,
        bg_r: u8,
        bg_g: u8,
        bg_b: u8,
        flags: u16,
    ) {
        self.inner.set_cell(
            col, row, char_str, width, fg_tag, fg_r, fg_g, fg_b, bg_tag, bg_r, bg_g, bg_b, flags,
        );
    }
    #[allow(clippy::too_many_arguments)]
    pub fn set_cell_ascii(
        &mut self,
        col: u16,
        row: u16,
        byte: u8,
        fg_tag: u8,
        fg_r: u8,
        fg_g: u8,
        fg_b: u8,
        bg_tag: u8,
        bg_r: u8,
        bg_g: u8,
        bg_b: u8,
        flags: u16,
    ) {
        self.inner.set_cell_ascii(
            col, row, byte, fg_tag, fg_r, fg_g, fg_b, bg_tag, bg_r, bg_g, bg_b, flags,
        );
    }
    pub fn get_cell_char(&self, col: u16, row: u16) -> String {
        self.inner.get_cell_char(col, row)
    }
    pub fn get_cell_width(&self, col: u16, row: u16) -> u8 {
        self.inner.get_cell_width(col, row)
    }
    pub fn get_cell_fg(&self, col: u16, row: u16) -> u32 {
        self.inner.get_cell_fg(col, row)
    }
    pub fn get_cell_bg(&self, col: u16, row: u16) -> u32 {
        self.inner.get_cell_bg(col, row)
    }
    pub fn get_cell_flags(&self, col: u16, row: u16) -> u16 {
        self.inner.get_cell_flags(col, row)
    }
    pub fn get_cell_hyperlink_id(&self, col: u16, row: u16) -> u16 {
        self.inner.get_cell_hyperlink_id(col, row)
    }
    pub fn get_hyperlink_uri(&self, id: u16) -> String {
        self.inner.get_hyperlink_uri(id)
    }
    pub fn get_hyperlink_params(&self, id: u16) -> String {
        self.inner.get_hyperlink_params(id)
    }
    pub fn grid_content_hash(&self) -> u32 {
        self.inner.grid_content_hash()
    }
    pub fn get_row_packed(&self, row: u16) -> Vec<u8> {
        self.inner.get_row_packed(row)
    }

    // ── Modes / dirty / tab stops ───────────────────────

    pub fn get_modes(&self) -> u32 {
        self.inner.get_modes()
    }
    pub fn set_modes(&mut self, modes: u32) {
        self.inner.set_modes(modes);
    }
    pub fn get_mode(&self, bit: u8) -> bool {
        self.inner.get_mode(bit)
    }
    pub fn set_mode(&mut self, bit: u8, value: bool) {
        self.inner.set_mode(bit, value);
    }
    pub fn set_tab_stop(&mut self, col: u16) {
        self.inner.set_tab_stop(col);
    }
    pub fn clear_tab_stop(&mut self, col: u16) {
        self.inner.clear_tab_stop(col);
    }
    pub fn clear_all_tab_stops(&mut self) {
        self.inner.clear_all_tab_stops();
    }
    pub fn next_tab_stop(&self, from_col: u16) -> u16 {
        self.inner.next_tab_stop(from_col)
    }
    pub fn get_dirty_rows(&self) -> Vec<u16> {
        self.inner.get_dirty_rows()
    }
    pub fn is_row_dirty(&self, row: u16) -> bool {
        self.inner.is_row_dirty(row)
    }
    pub fn mark_row_dirty(&mut self, row: u16) {
        self.inner.mark_row_dirty(row);
    }
    pub fn mark_all_dirty(&mut self) {
        self.inner.mark_all_dirty();
    }
    pub fn clear_dirty(&mut self) {
        self.inner.clear_dirty();
    }

    // ── Rows ────────────────────────────────────────────

    pub fn clear_line(&mut self, row: u16) {
        self.inner.clear_line(row);
    }
    pub fn clear_line_range(&mut self, row: u16, start_col: u16, end_col: u16) {
        self.inner.clear_line_range(row, start_col, end_col);
    }
    pub fn get_line_text(&self, row: u16) -> String {
        self.inner.get_line_text(row)
    }
    pub fn is_line_empty(&self, row: u16) -> bool {
        self.inner.is_line_empty(row)
    }
    pub fn get_line_wrapped(&self, row: u16) -> bool {
        self.inner.get_line_wrapped(row)
    }
    pub fn set_line_wrapped(&mut self, row: u16, wrapped: bool) {
        self.inner.set_line_wrapped(row, wrapped);
    }
    pub fn shift_rows_up(&mut self, start_row: u16, end_row: u16, count: u16) {
        self.inner.shift_rows_up(start_row, end_row, count);
    }
    pub fn shift_rows_down(&mut self, start_row: u16, end_row: u16, count: u16) {
        self.inner.shift_rows_down(start_row, end_row, count);
    }
    pub fn copy_row(&mut self, src_row: u16, dst_row: u16) {
        self.inner.copy_row(src_row, dst_row);
    }
    pub fn fill_row_default(&mut self, row: u16) {
        self.inner.fill_row_default(row);
    }

    // ── Scrollback ──────────────────────────────────────

    pub fn get_scrollback_length(&self) -> u32 {
        self.inner.get_scrollback_length()
    }
    pub fn get_scrollback_row_packed(&self, index: u32) -> Vec<u8> {
        self.inner.get_scrollback_row_packed(index)
    }
    pub fn get_scrollback_text(&self, index: u32) -> String {
        self.inner.get_scrollback_text(index)
    }
    pub fn get_scrollback_line_wrapped(&self, index: u32) -> bool {
        self.inner.get_scrollback_line_wrapped(index)
    }
    pub fn clear_scrollback(&mut self) {
        self.inner.clear_scrollback();
    }

    // ── Device responses ────────────────────────────────

    pub fn get_response_ptr(&self) -> u32 {
        self.inner.get_response_ptr() as u32
    }
    pub fn get_response_len(&self) -> u32 {
        self.inner.get_response_len()
    }
    pub fn get_response_bytes(&self) -> Vec<u8> {
        self.inner.get_response_bytes()
    }

    // ── Handlers (CSI / ESC / SGR / C0 / Print) ─────────

    pub fn handle_cursor_up(&mut self, count: u16) {
        self.inner.handle_cursor_up(count);
    }
    pub fn handle_cursor_down(&mut self, count: u16) {
        self.inner.handle_cursor_down(count);
    }
    pub fn handle_cursor_forward(&mut self, count: u16) {
        self.inner.handle_cursor_forward(count);
    }
    pub fn handle_cursor_back(&mut self, count: u16) {
        self.inner.handle_cursor_back(count);
    }
    pub fn handle_cursor_next_line(&mut self, count: u16) {
        self.inner.handle_cursor_next_line(count);
    }
    pub fn handle_cursor_previous_line(&mut self, count: u16) {
        self.inner.handle_cursor_previous_line(count);
    }
    pub fn handle_cursor_horizontal_absolute(&mut self, col: u16) {
        self.inner.handle_cursor_horizontal_absolute(col);
    }
    pub fn handle_cursor_position(&mut self, row: u16, col: u16) {
        self.inner.handle_cursor_position(row, col);
    }
    pub fn handle_cursor_vertical_absolute(&mut self, row: u16) {
        self.inner.handle_cursor_vertical_absolute(row);
    }

    pub fn handle_scroll_up(&mut self, count: u16) -> u8 {
        self.inner.handle_scroll_up(count)
    }
    pub fn handle_scroll_down(&mut self, count: u16) {
        self.inner.handle_scroll_down(count);
    }
    pub fn handle_decstbm(&mut self, top: u16, bottom: u16) {
        self.inner.handle_decstbm(top, bottom);
    }

    pub fn handle_erase_in_display(&mut self, mode: u8) -> u8 {
        self.inner.handle_erase_in_display(mode)
    }
    pub fn handle_erase_in_line(&mut self, mode: u8) {
        self.inner.handle_erase_in_line(mode);
    }
    pub fn handle_erase_characters(&mut self, count: u16) {
        self.inner.handle_erase_characters(count);
    }

    pub fn handle_set_mode(&mut self, mode: u16, enable: bool) -> u8 {
        self.inner.handle_set_mode(mode, enable)
    }

    pub fn handle_insert_lines(&mut self, count: u16) {
        self.inner.handle_insert_lines(count);
    }
    pub fn handle_delete_lines(&mut self, count: u16) {
        self.inner.handle_delete_lines(count);
    }
    pub fn handle_insert_characters(&mut self, count: u16) {
        self.inner.handle_insert_characters(count);
    }
    pub fn handle_delete_characters(&mut self, count: u16) {
        self.inner.handle_delete_characters(count);
    }

    pub fn handle_device_status_report(&mut self, ps: u8) -> u8 {
        self.inner.handle_device_status_report(ps)
    }
    pub fn handle_primary_device_attributes(&mut self) -> u8 {
        self.inner.handle_primary_device_attributes()
    }
    pub fn handle_secondary_device_attributes(&mut self) -> u8 {
        self.inner.handle_secondary_device_attributes()
    }

    pub fn handle_esc(&mut self, action: u8, data: u8) -> u8 {
        self.inner.handle_esc(action, data)
    }
    pub fn handle_execute(&mut self, byte: u8) -> u8 {
        self.inner.handle_execute(byte)
    }
    pub fn handle_sgr(&mut self, params: Vec<u16>) {
        self.inner.handle_sgr(&params);
    }
    pub fn handle_print(&mut self, cp: u32) -> u8 {
        self.inner.handle_print(cp)
    }

    pub fn process_pty_data(&mut self, data: &[u8]) -> u32 {
        self.inner.process_pty_data(data) as u32
    }

    // ── Snapshot ────────────────────────────────────────

    pub fn wasm_snapshot_to_bytes(&self) -> Vec<u8> {
        self.inner.snapshot_to_bytes()
    }

    pub fn wasm_restore_from_bytes(bytes: &[u8]) -> Option<TerminalCore> {
        let mut inner = term_core::TerminalCore::restore_from_bytes(bytes)?;
        // Re-install the JS bridge: snapshot deliberately does not preserve
        // callback handles (none of them are serializable), matching prior
        // behaviour.
        let callbacks = Self::install_callbacks(&mut inner);
        Some(TerminalCore { inner, callbacks })
    }

    // ── Debug ───────────────────────────────────────────

    pub fn wasm_debug_slim_stats(&self) -> JsValue {
        serde_wasm_bindgen::to_value(&self.inner.debug_slim_stats()).unwrap_or(JsValue::NULL)
    }
}

// ── WASM module entry point ─────────────────────────────

/// WASM module entry point — runs once per instance.
///
/// Installs `console_error_panic_hook` so any Rust panic surfaces in the
/// browser console (and the frontend log forwarder) with a Rust-side
/// stack trace instead of an opaque `RuntimeError: unreachable`.
///
/// Runs automatically on module instantiation, including on the reinit
/// path after a WASM crash recovery — every fresh instance gets the
/// hook installed without any JS-side coordination.
#[wasm_bindgen(start)]
pub fn wasm_main() {
    console_error_panic_hook::set_once();
}

// ── Free functions: parity with previous wasm exports ───

#[wasm_bindgen]
pub fn ping() -> u32 {
    term_core::ping()
}

#[wasm_bindgen]
pub fn char_width(cp: u32) -> u8 {
    term_core::char_width(cp)
}

#[wasm_bindgen]
pub fn classify_codepoint(cp: u32) -> u8 {
    term_core::classify_codepoint(cp)
}

#[wasm_bindgen]
pub fn is_emoji_presentation(cp: u32) -> bool {
    term_core::is_emoji_presentation(cp)
}

#[wasm_bindgen]
pub fn is_extended_pictographic(cp: u32) -> bool {
    term_core::is_extended_pictographic(cp)
}

#[wasm_bindgen]
pub fn is_regional_indicator(cp: u32) -> bool {
    term_core::is_regional_indicator(cp)
}

#[wasm_bindgen]
pub fn is_skin_tone_modifier(cp: u32) -> bool {
    term_core::is_skin_tone_modifier(cp)
}

#[wasm_bindgen]
pub fn is_variation_selector(cp: u32) -> bool {
    term_core::is_variation_selector(cp)
}

#[wasm_bindgen]
pub fn is_combining_char(cp: u32) -> bool {
    term_core::is_combining_char(cp)
}

#[wasm_bindgen]
pub fn is_ambiguous_width(cp: u32) -> bool {
    term_core::is_ambiguous_width(cp)
}

#[wasm_bindgen]
pub fn classify_codepoints(text: &str) -> Vec<u8> {
    term_core::classify_codepoints(text)
}

#[wasm_bindgen]
pub fn string_width(text: &str) -> u32 {
    term_core::string_width(text)
}
