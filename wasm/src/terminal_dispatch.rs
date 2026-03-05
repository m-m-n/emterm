/// PTY data dispatch: process_pty_data, dispatch_action, and buffer switch detection.
use crate::terminal_core::TerminalCore;

impl TerminalCore {
    /// Process raw PTY data through the WASM parser and dispatch internally.
    ///
    /// Uses take-dispatch-restore pattern to avoid intermediate Vec allocation:
    /// the parser is temporarily moved out of self so the parse callback can
    /// call dispatch methods on self without borrow conflicts.
    ///
    /// Returns the number of bytes consumed. If a buffer switch action is queued
    /// (mode 47/1047/1049), processing stops early so the caller can route
    /// remaining data to the correct core.
    pub fn process_pty_data(&mut self, data: &[u8]) -> usize {
        let mut parser = std::mem::take(&mut self.parser);
        let consumed = parser.parse_interruptible(data, |action| {
            self.dispatch_action(action);
            !self.has_pending_buffer_switch()
        });
        self.parser = parser;
        consumed
    }

    /// Check if mode_actions contains a pending buffer switch (action codes 1, 2, or 3).
    /// Skips TS_FALLBACK entries (3-byte: 0xFF/0xFE + mode_lo + mode_hi).
    pub(crate) fn has_pending_buffer_switch(&self) -> bool {
        let actions = &self.mode_actions;
        let mut i = 0;
        while i < actions.len() {
            let code = actions[i];
            if code == 0xFF || code == 0xFE {
                // TS_FALLBACK: 3-byte entry, skip
                i += 3;
            } else {
                if code >= 1 && code <= 3 {
                    return true;
                }
                i += 1;
            }
        }
        false
    }

    /// Route a single ParsedAction to the appropriate handler.
    fn dispatch_action(&mut self, action: crate::parser_types::ParsedAction) {
        use crate::parser_types::ParsedAction;
        match action {
            ParsedAction::Print(ch) => {
                self.handle_print(ch as u32);
            }
            // For all non-Print actions, flush the grapheme buffer first.
            // This ensures any accumulated emoji/pictographic codepoints are
            // written to the grid at the correct cursor position BEFORE
            // cursor movements, erases, or other operations change the state.
            ParsedAction::Execute(byte) => {
                if !self.grapheme_buffer.is_empty() {
                    self.flush_grapheme_buffer();
                }
                self.handle_execute_internal(byte);
            }
            ParsedAction::CsiDispatch {
                params,
                param_count,
                intermediates,
                intermediate_count,
                final_byte,
            } => {
                if !self.grapheme_buffer.is_empty() {
                    self.flush_grapheme_buffer();
                }
                self.handle_csi_internal(
                    &params[..param_count as usize],
                    &intermediates[..intermediate_count as usize],
                    final_byte,
                );
            }
            ParsedAction::EscDispatch {
                intermediate,
                final_byte,
            } => {
                if !self.grapheme_buffer.is_empty() {
                    self.flush_grapheme_buffer();
                }
                self.handle_esc_internal(intermediate, final_byte);
            }
            ParsedAction::OscDispatch { param, data } => {
                if !self.grapheme_buffer.is_empty() {
                    self.flush_grapheme_buffer();
                }
                self.handle_osc_internal(param, &data);
            }
            ParsedAction::ApcDispatch(payload) => {
                if !self.grapheme_buffer.is_empty() {
                    self.flush_grapheme_buffer();
                }
                use crate::apc_handler::KittyApcResult;
                let result = self.handle_kitty_apc(&payload);
                // Forward to backend for all except query (which needs no image processing)
                if !matches!(result, KittyApcResult::QueryHandled) {
                    self.fire_apc_callback(&payload);
                }
            }
            ParsedAction::DcsDispatch(payload) => {
                if !self.grapheme_buffer.is_empty() {
                    self.flush_grapheme_buffer();
                }
                self.fire_dcs_callback(&payload);
            }
        }
    }
}
