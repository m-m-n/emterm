/// Internal action types for parser output.
/// Not exported via wasm_bindgen - used only within the crate.

/// Maximum number of CSI parameters stored inline.
/// Must be >= 16 to handle combined SGR sequences like
/// `38;2;r;g;b;48;2;r;g;b;1;3;4m` (fg RGB + bg RGB + styles).
pub(crate) const MAX_CSI_PARAMS: usize = 16;
/// Maximum number of CSI intermediate bytes stored inline.
pub(crate) const MAX_CSI_INTERMEDIATES: usize = 2;

/// SC-1 (osc-color-query-response task0001): which string terminator ended
/// an OSC string, carried from the parser (`crate::parser::osc`) to the OSC
/// dispatch boundary (`crate::osc_handler::TerminalCore::handle_osc_internal`)
/// and on to any registered [`crate::osc_handler::OscResponder`].
///
/// SPEC assumption A6 resolution: the parser did NOT previously retain this
/// value — every `dispatch_osc` call site discarded which byte(s) ended the
/// string. This type and its carriage through [`ParsedAction::OscDispatch`]
/// is the retention this task adds.
///
/// `Unterminated` is the deterministic classification for every
/// [`ParsedAction::OscDispatch`] emitted WITHOUT a real terminator byte —
/// currently the sole such case: an ESC inside an OSC string is followed by
/// a byte other than `\` (cancelling ST formation), which dispatches the
/// accumulated string early and reprocesses that byte as a new escape
/// sequence (`Parser::osc_escape`'s fallback arm). A responder consulted for
/// an `Unterminated` dispatch is never told it was one of the two real
/// forms — `term_core` never manufactures a terminator match for a request
/// that was actually cut short.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OscTerminator {
    /// String terminated by BEL (0x07).
    Bel,
    /// String terminated by ST (`ESC \`).
    St,
    /// String ended by anything other than a real terminator.
    Unterminated,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum ParsedAction {
    Print(char),
    Execute(u8),
    CsiDispatch {
        params: [u16; MAX_CSI_PARAMS],
        param_count: u8,
        intermediates: [u8; MAX_CSI_INTERMEDIATES],
        intermediate_count: u8,
        final_byte: u8,
    },
    EscDispatch {
        intermediate: Option<u8>,
        final_byte: u8,
    },
    OscDispatch {
        param: u16,
        data: String,
        terminator: OscTerminator,
    },
    ApcDispatch(Vec<u8>),
    DcsDispatch(Vec<u8>),
}
