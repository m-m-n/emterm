/// Internal action types for parser output.
/// Not exported via wasm_bindgen - used only within the crate.

/// Maximum number of CSI parameters stored inline.
/// Must be >= 16 to handle combined SGR sequences like
/// `38;2;r;g;b;48;2;r;g;b;1;3;4m` (fg RGB + bg RGB + styles).
pub(crate) const MAX_CSI_PARAMS: usize = 16;
/// Maximum number of CSI intermediate bytes stored inline.
pub(crate) const MAX_CSI_INTERMEDIATES: usize = 2;

/// Which string terminator ended an OSC dispatch's string (SC-1,
/// osc-color-query-response IMPLEMENTATION.md). Every OSC dispatch carries
/// exactly one of these, derived from the bytes actually received; a
/// response producer echoes it back verbatim (FR5) instead of hardcoding a
/// terminator.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OscTerminator {
    /// `BEL` (0x07) ended the string.
    Bel,
    /// `ST` (`ESC \`) ended the string — including a string that was
    /// terminated ABNORMALLY: interrupted by an `ESC` that turned out to
    /// start a *different* escape sequence rather than complete `ESC \`
    /// (see `Parser::osc_escape`'s non-backslash arm). Classified as `St`
    /// here (the definition site, per SC-1's "classified deterministically
    /// ... documented at the definition site" requirement) because the
    /// byte actually seen was `ESC` — the same lead byte `ST` uses — and no
    /// `BEL` was ever received.
    St,
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
