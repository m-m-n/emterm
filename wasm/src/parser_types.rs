/// Internal action types for parser output.
/// Not exported via wasm_bindgen - used only within the crate.

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum ParsedAction {
    Print(char),
    Execute(u8),
    CsiDispatch {
        params: Vec<u16>,
        intermediates: Vec<u8>,
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
