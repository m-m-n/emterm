/// Parser state machine states.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) enum State {
    Ground,
    Escape,
    EscapeCharset(u8),
    CsiEntry,
    CsiParam,
    OscString,
    OscEscape,
    ApcString,
    ApcEscape,
    DcsString,
    DcsEscape,
}
