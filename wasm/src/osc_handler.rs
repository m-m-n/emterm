/// OSC internal dispatch: routes ParsedAction::OscDispatch to callbacks.
use crate::terminal_core::TerminalCore;

impl TerminalCore {
    pub(crate) fn handle_osc_internal(&mut self, param: u16, data: &str) {
        let action_type: u8 = match param {
            0 => 0,     // SetTitleAndIcon
            1 => 1,     // SetIconName
            2 => 2,     // SetTitle
            4 => 4,     // SetColorPalette
            7 => 7,     // SetWorkingDirectory
            8 => 8,     // Hyperlink
            10 => 10,   // SetForegroundColor
            11 => 11,   // SetBackgroundColor
            133 => 133, // SemanticPrompt
            777 => 100, // EmtermExtension (mapped to 100)
            _ => 255,   // Unknown
        };

        self.fire_osc_callback(action_type, data);
    }
}
