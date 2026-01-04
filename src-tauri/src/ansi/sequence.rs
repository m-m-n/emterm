//! Terminal action types for ANSI sequence parsing.
//!
//! This module defines the structured types that represent parsed ANSI sequences
//! and control characters. These actions are emitted by the parser and processed
//! by the terminal state machine.

use serde::Serialize;

use super::apc::ApcAction;
use super::dcs::DcsAction;

/// A terminal action emitted by the ANSI parser.
///
/// Each variant represents a different type of terminal operation that
/// the frontend should process.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "type", content = "value")]
pub enum TerminalAction {
    /// A printable character to display at the current cursor position.
    Print(char),

    /// A C0 control character (0x00-0x1F, excluding ESC).
    /// Common values: BEL (0x07), BS (0x08), HT (0x09), LF (0x0A), CR (0x0D).
    Execute(u8),

    /// A CSI (Control Sequence Introducer) sequence.
    Csi(CsiAction),

    /// An ESC (Escape) sequence.
    Esc(EscAction),

    /// An OSC (Operating System Command) sequence.
    Osc(OscAction),

    /// An APC (Application Program Command) sequence.
    /// Used for Kitty Graphics Protocol.
    Apc(ApcAction),

    /// A DCS (Device Control String) sequence.
    /// Used for SIXEL graphics.
    Dcs(DcsAction),
}

/// CSI (Control Sequence Introducer) actions.
///
/// CSI sequences start with ESC [ and are followed by parameters and a final byte.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "action", content = "data")]
pub enum CsiAction {
    /// SGR (Select Graphic Rendition) - CSI Ps m
    Sgr(Vec<u16>),

    /// Cursor Up - CSI Ps A
    CursorUp(u16),

    /// Cursor Down - CSI Ps B
    CursorDown(u16),

    /// Cursor Forward - CSI Ps C
    CursorForward(u16),

    /// Cursor Back - CSI Ps D
    CursorBack(u16),

    /// Cursor Next Line - CSI Ps E
    /// Move cursor down and to column 1.
    CursorNextLine(u16),

    /// Cursor Previous Line - CSI Ps F
    /// Move cursor up and to column 1.
    CursorPreviousLine(u16),

    /// Cursor Horizontal Absolute - CSI Ps G
    /// Move cursor to column Ps.
    CursorHorizontalAbsolute(u16),

    /// Cursor Position - CSI Ps ; Ps H or CSI Ps ; Ps f
    CursorPosition { row: u16, col: u16 },

    /// Cursor Vertical Absolute - CSI Ps d
    /// Move cursor to row Ps.
    CursorVerticalAbsolute(u16),

    /// Erase in Display - CSI Ps J
    EraseInDisplay(EraseMode),

    /// Erase in Line - CSI Ps K
    EraseInLine(EraseMode),

    /// Insert Lines - CSI Ps L
    /// Insert Ps blank lines at cursor row.
    InsertLines(u16),

    /// Delete Lines - CSI Ps M
    /// Delete Ps lines at cursor row.
    DeleteLines(u16),

    /// Insert Characters - CSI Ps @
    /// Insert Ps blank characters at cursor position.
    InsertCharacters(u16),

    /// Delete Characters - CSI Ps P
    /// Delete Ps characters at cursor position.
    DeleteCharacters(u16),

    /// Erase Characters - CSI Ps X
    /// Erase Ps characters at cursor position (fill with spaces).
    EraseCharacters(u16),

    /// Scroll Up - CSI Ps S
    /// Scroll up Ps lines within scroll region.
    ScrollUp(u16),

    /// Scroll Down - CSI Ps T
    /// Scroll down Ps lines within scroll region.
    ScrollDown(u16),

    /// Set Scroll Region (DECSTBM) - CSI Ps ; Ps r
    /// Set top and bottom margins of scroll region.
    SetScrollRegion { top: u16, bottom: u16 },

    /// Device Status Report - CSI Ps n
    /// - CSI 5 n: Device status (respond with CSI 0 n for OK)
    /// - CSI 6 n: Cursor position (respond with CSI row ; col R)
    DeviceStatusReport(u16),

    /// Primary Device Attributes - CSI c or CSI 0 c
    /// Response: CSI ? 64 ; Ps c (indicates VT420 compatible)
    PrimaryDeviceAttributes,

    /// Secondary Device Attributes - CSI > c or CSI > 0 c
    /// Response: CSI > Pp ; Pv ; Pc c (terminal type, version, ROM)
    SecondaryDeviceAttributes,

    /// Tertiary Device Attributes - CSI = c
    /// Response: DCS ! | text ST (device ID)
    TertiaryDeviceAttributes,

    /// Set Mode - CSI ? Ps h
    SetMode(Vec<u16>),

    /// Reset Mode - CSI ? Ps l
    ResetMode(Vec<u16>),

    /// Unrecognized or not-yet-implemented CSI sequence.
    /// Stores the final byte and parameters for debugging/future use.
    Unknown {
        params: Vec<u16>,
        intermediates: Vec<u8>,
        final_byte: u8,
    },
}

/// Erase mode for ED (Erase in Display) and EL (Erase in Line).
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize)]
pub enum EraseMode {
    /// Erase from cursor to end (default, Ps = 0).
    #[default]
    Below,
    /// Erase from start to cursor (Ps = 1).
    Above,
    /// Erase entire display/line (Ps = 2).
    All,
    /// Erase scrollback (ED only, Ps = 3).
    Scrollback,
}

impl From<u16> for EraseMode {
    fn from(value: u16) -> Self {
        match value {
            0 => Self::Below,
            1 => Self::Above,
            2 => Self::All,
            3 => Self::Scrollback,
            _ => Self::Below, // Default for unknown values
        }
    }
}

/// ESC (Escape) sequence actions.
///
/// These are simple escape sequences that don't use CSI or OSC format.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "action", content = "data")]
pub enum EscAction {
    /// ESC 7 - Save cursor position and attributes.
    SaveCursor,

    /// ESC 8 - Restore cursor position and attributes.
    RestoreCursor,

    /// ESC D - Index (move cursor down, scroll if at bottom).
    Index,

    /// ESC E - Next Line (move to column 0 of next line, scroll if needed).
    NextLine,

    /// ESC H - Horizontal Tab Set (set tab stop at current column).
    HorizontalTabSet,

    /// ESC M - Reverse Index (move cursor up, scroll if at top).
    ReverseIndex,

    /// ESC c - Reset to Initial State (full terminal reset).
    ResetToInitialState,

    /// ESC ( C - Select G0 Character Set.
    SetG0CharSet(CharSet),

    /// ESC ) C - Select G1 Character Set.
    SetG1CharSet(CharSet),

    /// Unrecognized escape sequence.
    Unknown(u8),
}

/// Character set designations for G0/G1 switching.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub enum CharSet {
    /// ASCII character set (B).
    Ascii,
    /// DEC Special Graphics / Line Drawing (0).
    DecLineDrawing,
    /// UK character set (A).
    Uk,
}

impl From<u8> for CharSet {
    fn from(value: u8) -> Self {
        match value {
            b'B' => Self::Ascii,
            b'0' => Self::DecLineDrawing,
            b'A' => Self::Uk,
            _ => Self::Ascii, // Default to ASCII for unknown
        }
    }
}

/// OSC (Operating System Command) actions.
///
/// OSC sequences start with ESC ] and are terminated by BEL or ST (ESC \).
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "action", content = "data")]
pub enum OscAction {
    /// OSC 0 - Set icon name and window title.
    SetTitleAndIcon(String),

    /// OSC 1 - Set icon name.
    SetIconName(String),

    /// OSC 2 - Set window title.
    SetTitle(String),

    /// OSC 4 - Set color palette entry.
    SetColorPalette { index: u8, color: String },

    /// OSC 7 - Set working directory.
    SetWorkingDirectory(String),

    /// OSC 8 - Hyperlink.
    Hyperlink { params: String, uri: String },

    /// OSC 10 - Query/Set foreground color.
    SetForegroundColor(String),

    /// OSC 11 - Query/Set background color.
    SetBackgroundColor(String),

    /// OSC 777 - eMterm extension (placeholder for future features).
    EmtermExtension { verb: String, params: Vec<String> },

    /// Unrecognized OSC sequence.
    Unknown { ps: u16, data: String },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_terminal_action_print() {
        let action = TerminalAction::Print('A');
        assert_eq!(action, TerminalAction::Print('A'));
    }

    #[test]
    fn test_terminal_action_execute() {
        let action = TerminalAction::Execute(0x0A); // LF
        assert_eq!(action, TerminalAction::Execute(0x0A));
    }

    #[test]
    fn test_csi_action_sgr() {
        let action = CsiAction::Sgr(vec![1, 31]);
        assert_eq!(action, CsiAction::Sgr(vec![1, 31]));
    }

    #[test]
    fn test_csi_action_cursor_position() {
        let action = CsiAction::CursorPosition { row: 10, col: 20 };
        if let CsiAction::CursorPosition { row, col } = action {
            assert_eq!(row, 10);
            assert_eq!(col, 20);
        } else {
            panic!("Expected CursorPosition");
        }
    }

    #[test]
    fn test_erase_mode_from_u16() {
        assert_eq!(EraseMode::from(0), EraseMode::Below);
        assert_eq!(EraseMode::from(1), EraseMode::Above);
        assert_eq!(EraseMode::from(2), EraseMode::All);
        assert_eq!(EraseMode::from(3), EraseMode::Scrollback);
        assert_eq!(EraseMode::from(99), EraseMode::Below); // Unknown defaults to Below
    }

    #[test]
    fn test_esc_action_variants() {
        assert_eq!(EscAction::SaveCursor, EscAction::SaveCursor);
        assert_eq!(EscAction::RestoreCursor, EscAction::RestoreCursor);
        assert_eq!(EscAction::Index, EscAction::Index);
    }

    #[test]
    fn test_charset_from_u8() {
        assert_eq!(CharSet::from(b'B'), CharSet::Ascii);
        assert_eq!(CharSet::from(b'0'), CharSet::DecLineDrawing);
        assert_eq!(CharSet::from(b'A'), CharSet::Uk);
        assert_eq!(CharSet::from(b'X'), CharSet::Ascii); // Unknown defaults to Ascii
    }

    #[test]
    fn test_osc_action_set_title() {
        let action = OscAction::SetTitle("My Terminal".to_string());
        if let OscAction::SetTitle(title) = action {
            assert_eq!(title, "My Terminal");
        } else {
            panic!("Expected SetTitle");
        }
    }

    #[test]
    fn test_terminal_action_serialization() {
        let action = TerminalAction::Print('A');
        let json = serde_json::to_string(&action).unwrap();
        assert!(json.contains("Print"));
    }

    #[test]
    fn test_csi_action_serialization() {
        let action = CsiAction::Sgr(vec![1, 31]);
        let json = serde_json::to_string(&action).unwrap();
        assert!(json.contains("Sgr"));
        assert!(json.contains("1"));
        assert!(json.contains("31"));
    }
}
