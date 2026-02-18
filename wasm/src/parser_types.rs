/// Internal action types for parser output.
/// Not exported via wasm_bindgen - used only within the crate.

/// Maximum number of CSI parameters stored inline.
pub(crate) const MAX_CSI_PARAMS: usize = 8;
/// Maximum number of CSI intermediate bytes stored inline.
pub(crate) const MAX_CSI_INTERMEDIATES: usize = 2;

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
    },
    ApcDispatch(Vec<u8>),
    DcsDispatch(Vec<u8>),
}
